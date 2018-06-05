use std::error;
use std::fs::File;
use std::io::Read;
use std::io;
use std::str;
use std::path::Path;
use std::fmt;
use futures::{future, Future};
use futures::stream::Stream;
use serde_json;
use serde::Serialize;
use serde::de::DeserializeOwned;
use hyper::{self, Body, Request, Uri};
use hyper::Method;
use hyper::StatusCode;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use hyper::header::{ContentType, ContentLength, Formatter, Header, Raw, Location as LocationHeader};
use hyper::Client as HttpClient;
use tokio_core::reactor::Handle;
use native_tls::{TlsConnector, Pkcs12, Certificate};
use error::{SwishClientError, RequestError};

#[derive(Debug)]
pub struct SwishClient {
    merchant_swish_number: String,
    swish_api_url: String,
    passphrase: String,
    cert_path: String,
    root_cert_path: String,
    handle: Handle,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatedPayment {
    pub id: String,
    pub location: String,
    pub request_token: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Payment {
    pub id: String,
    pub amount: f64,
    #[serde(rename = "payeePaymentReference")]
    pub payee_payment_reference: Option<String>,
    #[serde(rename = "paymentReference")]
    pub payment_reference: Option<String>,
    #[serde(rename = "payerAlias")]
    pub payer_alias: Option<String>,
    #[serde(rename = "payeeAlias")]
    pub payee_alias: Option<String>,

    pub message: Option<String>,
    pub status: Option<Status>,
    #[serde(rename = "dateCreated")]
    pub date_created: String,
    pub currency: Currency,
    #[serde(rename = "datePaid")]
    pub date_paid: Option<String>,

    // Errors can occur
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub enum Status {
    #[serde(rename = "CREATED")]
    Created,
    #[serde(rename = "PAID")]
    Paid,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "VALIDATED")]
    Validated,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentParams<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payee_payment_reference: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_alias: Option<&'a str>,
    pub payee_alias: &'a str,

    pub amount: f64,
    currency: Currency,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'a str>,
    pub callback_url: &'a str,
}

// impl<'a> PaymentParams<'a> {
//     fn new(payer_alias: &'a str, payee_alias: &'a str, amount: f64, callback_url: &'a str) -> Self {
//         PaymentParams {
//             payee_payment_reference: None,
//             payer_alias: Some(payer_alias),
//             payee_alias: payee_alias,
//             amount: amount,
//             message: None,
//             currency: Currency::default(),
//             callback_url: callback_url,
//         }
//     }
// }

#[derive(Debug, Default, Serialize)] 
#[serde(rename_all = "camelCase")]
pub struct RefundParams<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_payment_reference: Option<&'a str>,
    pub original_payment_reference: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_reference: Option<&'a str>,
    pub payer_alias: &'a str,
    pub payee_alias: &'a str,
    pub amount: f64,
    currency: Currency,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'a str>,
    pub callback_url: &'a str,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Currency {
    // SEK is the only one supported at Swish.
    SEK,
}

impl Default for Currency {
    fn default() -> Self {
        Currency::SEK
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatedRefund {
    pub id: String,
    pub location: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Refund {
    pub id: String,
    pub amount: f64,
    #[serde(rename = "payerPaymentReference")]
    pub payer_payment_reference: Option<String>,
    #[serde(rename = "originalpaymentReference")]
    pub original_payment_reference: Option<String>,
    #[serde(rename = "payerAlias")]
    pub payer_alias: Option<String>,
    #[serde(rename = "payeeAlias")]
    pub payee_alias: Option<String>,

    pub message: Option<String>,
    pub status: Option<Status>,
    #[serde(rename = "dateCreated")]
    pub date_created: String,
    pub currency: Currency,
    #[serde(rename = "datePaid")]
    pub date_paid: Option<String>,

    // Errors can occur
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
    #[serde(rename = "additionalInformation")]
    pub additional_information: Option<String>,
}

/// Custom Header returned by the Swish API
#[derive(Debug, Clone)]
struct PaymentRequestTokenHeader(String);

/// Custom Header implementation for the PaymentRequestToken
/// returned by the Swish API
impl Header for PaymentRequestTokenHeader {
    fn header_name() -> &'static str {
        "PaymentRequestToken"
    }

    fn parse_header(raw: &Raw) -> hyper::Result<PaymentRequestTokenHeader> {
        if raw.len() == 1 {
            let line = &raw[0];
            // Token is 32 characters.
            if line.len() == 32 {
                return Ok(PaymentRequestTokenHeader(String::from_utf8(line.to_vec())?));
            }
        }
        Err(hyper::Error::Header)
    }

    fn fmt_header(&self, f: &mut Formatter) -> fmt::Result {
        f.fmt_line(&self.0)
    }
}

/// Display implementation for the PaymentRequestToken
/// returned by the Swish API
impl fmt::Display for PaymentRequestTokenHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type alias for Future used within the SwishClient
type SwishBoxFuture<'a, T> = Box<Future<Item = T, Error = SwishClientError> + 'a>;

impl SwishClient {
    /// Creates a new SwishClient
    ///
    /// # Arguments
    ///
    /// * `merchant_swish_number` - The merchants swish number which will receive the payments
    /// * `cert_path` - The path to the certificate
    /// * `root_cert_path` - The path to the root certificate
    /// * `passphrase` - The passphrase to the certificate
    /// * `handle` - A tokio reactor handle
    ///
    /// # Example
    ///
    /// ```
    /// use swish::client::SwishClient;
    /// use tokio_core::reactor::Core;
    ///
    /// let core = Core::new().unwrap();
    /// let handle = core.handle();
    /// let current_dir = env::current_dir()?;
    /// let cert_path = current_dir.join("./certs/test_cert.p12");
    /// let root_cert_path = current_dir.join("./certs/root_cert.der");
    /// let swish_client = cert_path
    ///     .into_os_string()
    ///     .to_str()
    ///     .and_then(|cert_path_string| {
    ///         root_cert_path
    ///             .into_os_string()
    ///             .to_str()
    ///             .map(|root_cert_path_string| {
    ///                      client::SwishClient::new("1231181189",
    ///                                               cert_path_string,
    ///                                               root_cert_path_string,
    ///                                               "passphrase",
    ///                                               handle)
    ///                  })
    ///     });
    /// ```
    pub fn new(merchant_swish_number: &str,
               cert_path: &str,
               root_cert_path: &str,
               passphrase: &str,
               handle: Handle)
               -> Self {
        SwishClient {
            merchant_swish_number: merchant_swish_number.to_owned(),
            swish_api_url: "https://mss.swicpc.bankgirot.se/swish-cpcapi/api/v1/".to_owned(),
            passphrase: passphrase.to_owned(),
            cert_path: cert_path.to_owned(),
            root_cert_path: root_cert_path.to_owned(),
            handle: handle,
        }
    }

    /// Creates a payment with the provided PaymentParams.
    /// Returns a Future with a CreatedPayment.
    ///
    /// # Arguments
    ///
    /// * `params` - PaymentParams
    ///
    /// # Example
    ///
    /// ```
    /// use swish::client::SwishClient;
    ///
    /// let mut payment_params = client::PaymentParams::default();
    /// payment_params.amount = 100.00;
    /// payment_params.payee_alias = "1231181189";
    /// payment_params.payee_payment_reference = Some("0123456789");
    /// payment_params.callback_url = "https://example.com/api/swishcb/paymentrequests";
    /// payment_params.message = Some("Kingston USB Flash Drive 8 GB");
    ///
    /// let payment = client.create_payment(payment_params);
    /// ```
    pub fn create_payment<'a>(&'a self,
                              params: PaymentParams)
                              -> SwishBoxFuture<'a, CreatedPayment> {
        let payment_params = PaymentParams {
            payee_alias: self.merchant_swish_number.as_str(),
            ..params
        };

        let response: SwishBoxFuture<'a, (String, hyper::Headers)> =
            self.post::<CreatedPayment, PaymentParams>("paymentrequests", payment_params);

        let payment_future = response.and_then(move |(_, headers)| {
            let location = get_header_as_string::<LocationHeader>(&headers);
            let request_token = get_header_as_string::<PaymentRequestTokenHeader>(&headers);

            let payment = location.and_then(|location| {
                self.get_payment_id_from_location(location.to_owned())
                    .map(|payment_id| {
                             CreatedPayment {
                                 id: payment_id,
                                 request_token: request_token,
                                 location: location,
                             }
                         })
            });

            future::result(serde_json::from_value(json!(payment))
                                              .map_err(|err| SwishClientError::from(err)))
        });
        Box::new(payment_future)
    }

    /// Gets a payment for a given payment_id.
    /// Returns a Future with a Payment.
    ///
    /// # Arguments
    ///
    /// * `payment_id` - A string id for a payment
    ///
    /// # Example
    ///
    /// ```
    /// use swish::client::SwishClient;
    ///
    /// let payment_id = "111";
    /// let payment = client.get_payment(payment_id.as_str())
    /// ```
    pub fn get_payment<'a>(&'a self, payment_id: &str) -> SwishBoxFuture<'a, Payment> {
        self.get(format!("paymentrequests/{}", payment_id).as_str())
    }

    /// Creates a refund with the provided RefundParams.
    /// Returns a Future with a CreatedRefund.
    ///
    /// # Arguments
    ///
    /// * `params` - RefundParams
    ///
    /// # Example
    ///
    /// ```
    /// use swish::client::SwishClient;
    ///
    /// let mut refund_params = client::RefundParams::default();
    /// refund_params.amount = 100.00;
    /// refund_params.callback_url = "https://example.com/api/swishcb/refunds";
    /// refund_params.original_payment_reference = payment_reference.as_str();
    /// refund_params.payer_payment_reference = Some("0123456789");
    /// refund_params.message = Some("Refund for Kingston USB Flash Drive 8 GB");
    ///
    /// let refund = client.create_refund(refund_params);
    /// ```
    pub fn create_refund<'a>(&'a self, params: RefundParams) -> SwishBoxFuture<'a, CreatedRefund> {
        let refund_params = RefundParams {
            payer_alias: self.merchant_swish_number.as_str(),
            ..params
        };

        let response = self.post::<CreatedRefund, RefundParams>("refunds", refund_params);

        let refund_future = response.and_then(move |(_, headers)| {
            let location = get_header_as_string::<LocationHeader>(&headers);

            let refund = location.and_then(|location| {
                self.get_payment_id_from_location(location.to_owned())
                    .map(|refund_id| {
                             CreatedRefund {
                                 id: refund_id,
                                 location: location,
                             }
                         })
            });

            future::result(serde_json::from_value(json!(refund))
                                              .map_err(|err| SwishClientError::from(err)))
        });
        Box::new(refund_future)
    }

    /// Gets a refund for a given refund_id.
    /// Returns a Future with a Refund.
    ///
    /// # Arguments
    ///
    /// * `refund_id` - A string id for a refund
    ///
    /// # Example
    ///
    /// ```
    /// use swish::client::SwishClient;
    ///
    /// let refund_id = "111";
    /// let refund = client.get_refund(refund_id.as_str())
    /// ```
    pub fn get_refund<'a>(&'a self, refund_id: &str) -> SwishBoxFuture<'a, Refund> {
        self.get(format!("refunds/{}", refund_id).as_str())
    }

    /// Reads a given cert into a Vec.
    /// Returns a Result that contains the Vec if it succeeded.
    ///
    /// # Arguments
    ///
    /// * `cert_path` - A string path to the place where the cert is
    fn read_cert(&self, cert_path: &str) -> Result<Vec<u8>, io::Error> {
        let cert_path = Path::new(&cert_path);
        let mut buf = vec![];
        let _result = File::open(cert_path).and_then(|mut f| f.read_to_end(&mut buf));
        Ok(buf)
    }

    /// Build a HTTPS client with the root_cert and the client_cert.
    /// Returns a Result that contains the client if it succeeded.
    fn build_client
        (&self)
         -> Result<HttpClient<HttpsConnector<HttpConnector>, Body>, Box<error::Error>> {
        let root_cert = Certificate::from_der(&self.read_cert(&self.root_cert_path)?)?;
        let client_cert = Pkcs12::from_der(&self.read_cert(&self.cert_path)?, &self.passphrase)?;

        let mut builder = try!(TlsConnector::builder());
        try!(builder.add_root_certificate(root_cert));
        try!(builder.identity(client_cert));
        let built_connector: TlsConnector = try!(builder.build());

        let mut http_connector = HttpConnector::new(4, &self.handle);
        http_connector.enforce_http(false);

        let https_connector = HttpsConnector::from((http_connector, built_connector));

        let client = HttpClient::configure()
            .connector(https_connector)
            .build(&self.handle);

        Ok(client)
    }

    /// Performs a http POST request to the Swish API.
    /// Returns a Future with a Tuple that contains the body as a String
    /// and the Response headers.
    ///
    /// # Arguments
    ///
    /// * `path` - A string path
    /// * `params` - Params that implements Serialize which are json sent as the body
    fn post<'a, T: 'a, P>(&'a self,
                          path: &str,
                          params: P)
                          -> SwishBoxFuture<'a, (String, hyper::Headers)>
        where T: DeserializeOwned + fmt::Debug,
              P: Serialize
    {
        let future_result: Result<_, SwishClientError> = self.get_uri(path)
            .and_then(|uri| {
                serde_json::to_string(&params)
                    .and_then(|json_params| {
                        let mut request = Request::new(Method::Post, uri.clone());
                        request.headers_mut().set(ContentType::json());
                        request
                            .headers_mut()
                            .set(ContentLength(json_params.len() as u64));
                        request.set_body(json_params.clone());

                        println!("Posting JSON: {:?} at URI: {:?}", json_params, uri);
                        Ok(self.perform_swish_api_request(request))
                    })
                    .map_err(SwishClientError::from)
            })
            .and_then(|future| Ok(future));
        Box::new(future::result(future_result).flatten())
    }

    /// Performs a http GET request to the Swish API.
    /// Returns a Future with a Tuple that contains the body as a String
    /// and the Response headers.
    ///
    /// # Arguments
    ///
    /// * `path` - A string path
    fn get<'a, T: 'a>(&'a self, path: &str) -> SwishBoxFuture<'a, T>
        where T: DeserializeOwned + fmt::Debug
    {
        let uri = self.get_uri(path).unwrap();
        let request = Request::new(Method::Get, uri.clone());

        println!("Getting URI: {:?}", uri);
        let future = self.perform_swish_api_request(request)
            .and_then(move |(body, _)| future::result(self.parse_body::<T>(&body)));
        Box::new(future)
    }

    /// Parse a body as json.
    ///
    /// # Arguments
    ///
    /// * `body` - A string body
    fn parse_body<T>(&self, body: &str) -> Result<T, SwishClientError>
        where T: DeserializeOwned + fmt::Debug
    {
        serde_json::from_str(&body).map_err(|err| SwishClientError::from(err))
    }

    /// Parse a given string path into an Uri.
    ///
    /// # Arguments
    ///
    /// * `path` - A string path
    fn get_uri(&self, path: &str) -> Result<Uri, SwishClientError> {
        format!("{}{}", self.swish_api_url, path)
            .parse::<Uri>()
            .map_err(|err| SwishClientError::from(err))
    }

    /// Gets a payment_id from a given location header string.
    ///
    /// # Arguments
    ///
    /// * `location` - A string location header
    fn get_payment_id_from_location(&self, location: String) -> Option<String> {
        let payment_id: Vec<&str> = location.split('/').collect();
        payment_id.last().cloned().map(|id| id.to_string())
    }

    /// Performs the actual request to the Swish API.
    /// Returns a Future with a Tuple that contains the body as a String
    /// and the Response headers.
    fn perform_swish_api_request<'a>(&'a self,
                                     request: Request)
                                     -> SwishBoxFuture<'a, (String, hyper::Headers)> {
        let client =
            self.build_client()
                .expect("The HttpsClient couldn't be built, the certificate is probably wrong");
        let future = client
            .request(request)
            .map_err(|err| SwishClientError::from(err))   
            .and_then(move |response| {
                let status = response.status();
                let headers = response.headers().clone();

                response.body().concat2()
                    .map_err(|err| SwishClientError::from(err))
                    .and_then(move |body| {
                        let body = str::from_utf8(&body).unwrap();
                        if status == StatusCode::NotFound {
                            let error = RequestError {
                                http_status: StatusCode::NotFound,
                                code: None,
                                additional_information: None,
                                message: body.to_owned(),
                            };
                            return future::err(SwishClientError::from(error));
                        }

                        if !status.is_success() {
                            // Swish can sometimes return an array of errors.
                            // TODO: http_status is wrong, set it to the actually status.
                            let errors: SwishClientError = match serde_json::from_str::<serde_json::Value>(body) {
                                Ok(json) => {
                                    let errors: Vec<_> = json.as_array()
                                    .into_iter()
                                    .flat_map(|e| {
                                        e.iter()
                                            .flat_map(|err| serde_json::from_value::<RequestError>(err.clone()))
                                            .map(|request_error| {
                                                RequestError {http_status_code: status, ..request_error}
                                            })
                                            .map(SwishClientError::from) 
                                            .collect::<Vec<SwishClientError>>()
                                    })
                                    .collect();
                                    SwishClientError::from(errors)
                                }, 
                                Err(err) => {
                                    let error = RequestError {
                                        additional_information: None, code: None, http_status: status, message: err.to_string()
                                    };
                                    SwishClientError::from(error)
                                }
                            };
                            return future::err(errors)
                        }
                        future::result(Ok((body.to_owned(), headers))) 
                    })
            });
        Box::new(future)
    }
}

/// Gets a hyper::Header and turns it into a String.
///
/// # Arguments
///
/// * `headers` - hyper::Headers
fn get_header_as_string<T: Header + fmt::Display>(headers: &hyper::Headers) -> Option<String> {
    headers.get::<T>().map(|h| h.to_string())
}