//! # The SwishClient
//!
//! This is the client that's used to make calls to the Swish API.
//!
use error::{RequestError, SwishClientError};
use futures::stream::Stream;
use futures::{future, Future};
use hyper::client::HttpConnector;
use hyper::header::{self, HeaderValue, CONTENT_TYPE, LOCATION};
use hyper::Client as HttpClient;
use hyper::StatusCode;
use hyper::{self, Body, Request, Uri};
use hyper_tls::HttpsConnector;
use native_tls::{Certificate, Identity, TlsConnector};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json;
use std::error;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::Path;
use std::str;
use tokio_core::reactor::Handle;

/// The client used to make call to the Swish API.
#[derive(Debug)]
pub struct SwishClient {
    merchant_swish_number: String,
    swish_api_url: String,
    passphrase: String,
    cert_path: String,
    root_cert_path: String,
    handle: Handle,
}

/// This is what will be returned when a payment is
/// successfully created at Swish.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreatedPayment {
    pub id: String,
    pub location: String,
    pub request_token: Option<String>,
}

/// This is all the data that's returned from the
/// Swish API when fetching a payment.
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

/// The status of an operation.
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
    #[serde(rename = "INITIATED")]
    Initiated,
}

/// Params used to create a new payment.
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

/// Params used to create a new refund.
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

/// The currency the Swish API supports.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Currency {
    /// SEK is currently the only currency supported at Swish.
    SEK,
}

impl Default for Currency {
    fn default() -> Self {
        Currency::SEK
    }
}

/// This will be returned when a refund
/// is successfully created.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreatedRefund {
    pub id: String,
    pub location: String,
}

/// This is all the data that's returned
/// from the Swish API when fetching a refund.
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

/// Custom Header returned by the Swish API.
const PAYMENT_REQUEST_TOKEN: &'static str = "paymentrequesttoken";

/// Type alias for Future used within the SwishClient
type SwishBoxFuture<'a, T> = Box<Future<Item = T, Error = SwishClientError> + 'a>;

impl SwishClient {
    /// [`SwishClient`]: struct.SwishClient.html
    ///
    /// Creates a new SwishClient
    ///
    /// # Arguments
    ///
    /// * `merchant_swish_number` - The merchants swish number which will receive the payments.
    /// * `cert_path` - The path to the certificate.
    /// * `root_cert_path` - The path to the root certificate.
    /// * `passphrase` - The passphrase to the certificate.
    /// * `handle` - A tokio reactor handle.
    ///
    /// # Returns
    /// A configured [`SwishClient`].
    ///
    /// # Example
    ///
    /// ```
    /// extern crate tokio_core;
    /// extern crate swish;
    ///
    /// use swish::client::SwishClient;
    /// use tokio_core::reactor::Core;
    /// use std::env;
    ///
    /// let core = Core::new().unwrap();
    /// let handle = core.handle();
    /// let current_dir = env::current_dir().unwrap();
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
    ///                      SwishClient::new("1231181189",
    ///                                               cert_path_string,
    ///                                               root_cert_path_string,
    ///                                               "passphrase",
    ///                                               handle)
    ///                  })
    ///     });
    /// ```
    pub fn new(
        merchant_swish_number: &str,
        cert_path: &str,
        root_cert_path: &str,
        passphrase: &str,
        handle: Handle,
    ) -> Self {
        SwishClient {
            merchant_swish_number: merchant_swish_number.to_owned(),
            swish_api_url: "https://mss.cpc.getswish.net/swish-cpcapi/api/v1/".to_owned(),
            passphrase: passphrase.to_owned(),
            cert_path: cert_path.to_owned(),
            root_cert_path: root_cert_path.to_owned(),
            handle: handle,
        }
    }

    /// [`PaymentParams`]: struct.PaymentParams.html
    /// [`CreatedPayment`]: struct.CreatedPayment.html
    ///
    /// Creates a payment with the provided [`PaymentParams`].
    ///
    /// # Returns
    /// A Future with a [`CreatedPayment`].
    ///
    /// # Arguments
    ///
    /// * `params` - [`PaymentParams`].
    ///
    /// # Example
    ///
    /// ```
    /// extern crate tokio_core;
    /// extern crate swish;
    ///
    /// use tokio_core::reactor::Core;
    /// use std::env;
    /// use swish::client::{PaymentParams, SwishClient};
    ///
    /// let core = Core::new().unwrap();
    /// let handle = core.handle();
    /// let current_dir = env::current_dir().unwrap();
    /// let cert_path = current_dir.join("./tests/test_cert.p12");
    /// let root_cert_path = current_dir.join("./tests/root_cert.der");
    /// let swish_client = cert_path
    ///     .into_os_string()
    ///     .to_str()
    ///     .and_then(|cert_path_string| {
    ///         root_cert_path
    ///             .into_os_string()
    ///             .to_str()
    ///             .map(|root_cert_path_string| {
    ///                 SwishClient::new(
    ///                     "1231181189",
    ///                     cert_path_string,
    ///                     root_cert_path_string,
    ///                     "swish",
    ///                     handle,
    ///                 )
    ///             })
    ///     }).unwrap();
    ///
    /// let mut payment_params = PaymentParams::default();
    /// payment_params.amount = 100.00;
    /// payment_params.payee_alias = "1231181189";
    /// payment_params.payee_payment_reference = Some("0123456789");
    /// payment_params.callback_url = "https://example.com/api/swishcb/paymentrequests";
    /// payment_params.message = Some("Kingston USB Flash Drive 8 GB");
    ///
    /// let payment = swish_client.create_payment(payment_params);
    ///
    /// ```
    pub fn create_payment<'a>(
        &'a self,
        params: PaymentParams,
    ) -> SwishBoxFuture<'a, CreatedPayment> {
        let payment_params = PaymentParams {
            payee_alias: self.merchant_swish_number.as_str(),
            ..params
        };

        let response: SwishBoxFuture<'a, (String, header::HeaderMap)> =
            self.post::<CreatedPayment, PaymentParams>("paymentrequests", payment_params);

        let payment_future = response.and_then(move |(_, headers)| {
            let location = get_header_as_string(&headers, &LOCATION);
            let request_token = get_header_as_string(
                &headers,
                &header::HeaderName::from_static(PAYMENT_REQUEST_TOKEN),
            );

            let payment = location.and_then(|location| {
                self.get_payment_id_from_location(location.to_owned())
                    .map(|payment_id| CreatedPayment {
                        id: payment_id,
                        request_token,
                        location,
                    })
            });

            future::result(
                serde_json::from_value(json!(payment)).map_err(|err| SwishClientError::from(err)),
            )
        });
        Box::new(payment_future)
    }

    /// [`Payment`]: struct.Payment.html
    ///
    /// Gets a payment for a given `payment_id`.
    ///
    /// # Returns
    /// A Future with a [`Payment`].
    ///
    /// # Arguments
    ///
    /// * `payment_id` - A string id for a payment
    ///
    /// # Example
    ///
    /// ```
    /// extern crate tokio_core;
    /// extern crate swish;
    ///
    /// use tokio_core::reactor::Core;
    /// use std::env;
    /// use swish::client::SwishClient;
    ///
    /// let core = Core::new().unwrap();
    /// let handle = core.handle();
    /// let current_dir = env::current_dir().unwrap();
    /// let cert_path = current_dir.join("./tests/test_cert.p12");
    /// let root_cert_path = current_dir.join("./tests/root_cert.der");
    /// let swish_client = cert_path
    ///     .into_os_string()
    ///     .to_str()
    ///     .and_then(|cert_path_string| {
    ///         root_cert_path
    ///             .into_os_string()
    ///             .to_str()
    ///             .map(|root_cert_path_string| {
    ///                 SwishClient::new(
    ///                     "1231181189",
    ///                     cert_path_string,
    ///                     root_cert_path_string,
    ///                     "swish",
    ///                     handle,
    ///                 )
    ///             })
    ///     }).unwrap();
    ///
    /// let payment_id = "111";
    /// let payment = swish_client.get_payment(payment_id);
    /// ```
    pub fn get_payment<'a>(&'a self, payment_id: &str) -> SwishBoxFuture<'a, Payment> {
        self.get(format!("paymentrequests/{}", payment_id).as_str())
    }

    /// [`RefundParams`]: struct.RefundParams.html
    /// [`CreatedRefund`]: struct.CreatedRefund.html
    ///
    /// Creates a refund with the provided [`RefundParams`].
    ///
    /// # Returns
    /// A Future with a [`CreatedRefund`].
    ///
    /// # Arguments
    ///
    /// * `params` - [`RefundParams`].
    ///
    /// # Example
    ///
    /// ```
    /// extern crate tokio_core;
    /// extern crate swish;
    ///
    /// use tokio_core::reactor::Core;
    /// use std::env;
    /// use swish::client::{RefundParams, SwishClient};
    ///
    /// let core = Core::new().unwrap();
    /// let handle = core.handle();
    /// let current_dir = env::current_dir().unwrap();
    /// let cert_path = current_dir.join("./tests/test_cert.p12");
    /// let root_cert_path = current_dir.join("./tests/root_cert.der");
    /// let swish_client = cert_path
    ///     .into_os_string()
    ///     .to_str()
    ///     .and_then(|cert_path_string| {
    ///         root_cert_path
    ///             .into_os_string()
    ///             .to_str()
    ///             .map(|root_cert_path_string| {
    ///                 SwishClient::new(
    ///                     "1231181189",
    ///                     cert_path_string,
    ///                     root_cert_path_string,
    ///                     "swish",
    ///                     handle,
    ///                 )
    ///             })
    ///     }).unwrap();
    ///
    /// let mut refund_params = RefundParams::default();
    /// refund_params.amount = 100.00;
    /// refund_params.callback_url = "https://example.com/api/swishcb/refunds";
    /// refund_params.payer_payment_reference = Some("0123456789");
    /// refund_params.message = Some("Refund for Kingston USB Flash Drive 8 GB");
    ///
    /// let refund = swish_client.create_refund(refund_params);
    /// ```
    pub fn create_refund<'a>(&'a self, params: RefundParams) -> SwishBoxFuture<'a, CreatedRefund> {
        let refund_params = RefundParams {
            payer_alias: self.merchant_swish_number.as_str(),
            ..params
        };

        let response = self.post::<CreatedRefund, RefundParams>("refunds", refund_params);

        let refund_future = response.and_then(move |(_, headers)| {
            let location = get_header_as_string(&headers, &LOCATION);

            let refund = location.and_then(|location| {
                self.get_payment_id_from_location(location.to_owned())
                    .map(|refund_id| CreatedRefund {
                        id: refund_id,
                        location: location,
                    })
            });

            future::result(
                serde_json::from_value(json!(refund)).map_err(|err| SwishClientError::from(err)),
            )
        });
        Box::new(refund_future)
    }

    /// [`Refund`]: struct.Refund.html
    ///
    /// Gets a refund for a given `refund_id`.
    ///
    /// # Returns
    /// A Future with a [`Refund`].
    ///
    /// # Arguments
    ///
    /// * `refund_id` - A string id for a refund
    ///
    /// # Example
    ///
    /// ```
    /// extern crate tokio_core;
    /// extern crate swish;
    ///
    /// use tokio_core::reactor::Core;
    /// use std::env;
    /// use swish::client::SwishClient;
    ///
    /// let core = Core::new().unwrap();
    /// let handle = core.handle();
    /// let current_dir = env::current_dir().unwrap();
    /// let cert_path = current_dir.join("./tests/test_cert.p12");
    /// let root_cert_path = current_dir.join("./tests/root_cert.der");
    /// let swish_client = cert_path
    ///     .into_os_string()
    ///     .to_str()
    ///     .and_then(|cert_path_string| {
    ///         root_cert_path
    ///             .into_os_string()
    ///             .to_str()
    ///             .map(|root_cert_path_string| {
    ///                 SwishClient::new(
    ///                     "1231181189",
    ///                     cert_path_string,
    ///                     root_cert_path_string,
    ///                     "swish",
    ///                     handle,
    ///                 )
    ///             })
    ///     }).unwrap();
    ///
    /// let refund_id = "111";
    /// let refund = swish_client.get_refund(refund_id);
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
    /// # Returns
    /// A Result that contains the client if it succeeded.
    fn build_client(
        &self,
    ) -> Result<HttpClient<HttpsConnector<HttpConnector>, Body>, Box<error::Error>> {
        let _root_cert = Certificate::from_der(&self.read_cert(&self.root_cert_path)?)?;
        let pkcs12_cert = &self.read_cert(&self.cert_path)?;
        let client_cert = Identity::from_pkcs12(&pkcs12_cert, &self.passphrase)?;

        let tls_connector = TlsConnector::builder()
            //.add_root_certificate(root_cert)
            .identity(client_cert)
            .build()?;

        let mut http_connector = HttpConnector::new(4);
        http_connector.enforce_http(false);

        let https_connector = HttpsConnector::from((http_connector, tls_connector));

        let client = hyper::client::Client::builder().build(https_connector);

        Ok(client)
    }

    /// Performs a http POST request to the Swish API.
    ///
    /// # Returns
    /// A Future with a Tuple that contains the body as a String
    /// and the Response headers.
    ///
    /// # Arguments
    ///
    /// * `path` - A string path
    /// * `params` - Params that implements Serialize which are json sent as the body
    fn post<'a, T: 'a, P>(
        &'a self,
        path: &str,
        params: P,
    ) -> SwishBoxFuture<'a, (String, hyper::header::HeaderMap)>
    where
        T: DeserializeOwned + fmt::Debug,
        P: Serialize,
    {
        let future_result: Result<_, SwishClientError> = self
            .get_uri(path)
            .and_then(|uri| {
                serde_json::to_string(&params)
                    .and_then(|json_params| {
                        let mut request = Request::post(uri.to_owned())
                            .body(Body::from(json_params))
                            .unwrap();
                        request
                            .headers_mut()
                            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

                        Ok(self.perform_swish_api_request(request))
                    }).map_err(SwishClientError::from)
            }).and_then(|future| Ok(future));
        Box::new(future::result(future_result).flatten())
    }

    /// Performs a http GET request to the Swish API.
    ///
    /// # Returns
    /// A Future with a Tuple that contains the body as a String
    /// and the Response headers.
    ///
    /// # Arguments
    ///
    /// * `path` - A string path
    fn get<'a, T: 'a>(&'a self, path: &str) -> SwishBoxFuture<'a, T>
    where
        T: DeserializeOwned + fmt::Debug,
    {
        let uri = self.get_uri(path).unwrap();
        let request = Request::get(uri).body(Body::empty()).unwrap();

        let future = self
            .perform_swish_api_request(request)
            .and_then(move |(body, _)| future::result(self.parse_body::<T>(&body)));
        Box::new(future)
    }

    /// Parse body as json.
    ///
    /// # Arguments
    ///
    /// * `body` - A string body
    fn parse_body<T>(&self, body: &str) -> Result<T, SwishClientError>
    where
        T: DeserializeOwned + fmt::Debug,
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
    /// # Returns
    /// A Future with a Tuple that contains the body as a String
    /// and the Response headers.
    fn perform_swish_api_request<'a>(
        &'a self,
        request: Request<hyper::Body>,
    ) -> SwishBoxFuture<'a, (String, hyper::HeaderMap)> {
        let client = self
            .build_client()
            .expect("The HttpsClient couldn't be built, the certificate is probably wrong");

        let future = client
            .request(request)
            .map_err(|err| SwishClientError::from(err))
            .and_then(move |response| {
                let status = response.status();
                let headers = response.headers().to_owned();

                response
                    .into_body()
                    .concat2()
                    .map_err(|err| SwishClientError::from(err))
                    .and_then(move |body| {
                        let body = str::from_utf8(&body).unwrap();

                        if status == StatusCode::NOT_FOUND {
                            let error = RequestError {
                                http_status: StatusCode::NOT_FOUND,
                                code: None,
                                additional_information: None,
                                message: body.to_owned(),
                            };
                            return future::err(SwishClientError::from(error));
                        }

                        if !status.is_success() {
                            // Swish can sometimes return an array of errors.
                            // TODO: http_status is wrong, set it to the actually status.
                            let errors: SwishClientError =
                                match serde_json::from_str::<serde_json::Value>(body) {
                                    Ok(json) => {
                                        let errors: Vec<_> = json
                                            .as_array()
                                            .into_iter()
                                            .flat_map(|e| {
                                                e.iter()
                                                    .flat_map(|err| {
                                                        serde_json::from_value::<RequestError>(
                                                            err.clone(),
                                                        )
                                                    }).map(|request_error| RequestError {
                                                        http_status: status,
                                                        ..request_error
                                                    }).map(SwishClientError::from)
                                                    .collect::<Vec<SwishClientError>>()
                                            }).collect();
                                        SwishClientError::from(errors)
                                    }
                                    Err(err) => {
                                        let error = RequestError {
                                            additional_information: None,
                                            code: None,
                                            http_status: status,
                                            message: err.to_string(),
                                        };
                                        SwishClientError::from(error)
                                    }
                                };
                            return future::err(errors);
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
fn get_header_as_string(
    headers: &hyper::header::HeaderMap,
    header: &hyper::header::HeaderName,
) -> Option<String> {
    headers
        .get(header)
        .and_then(|h| h.to_str().ok().map(|h| h.to_string()))
}
