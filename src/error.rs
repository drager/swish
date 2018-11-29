//! # The error module
//!
//! Contains all the errors that can occur.
//!
extern crate hyper;
extern crate serde_json;

use hyper::http::uri;
use std::fmt;
use std::io;

pub type ErrorCollection = Vec<SwishClientError>;
#[derive(Debug)]
pub enum SwishClientError {
    Swish(RequestError),
    Parse(String),
    Http(hyper::Error),
    Uri(uri::InvalidUri),
    Io(io::Error),
    Json(serde_json::Error),
    ErrorCollection(ErrorCollection),
}

impl fmt::Display for SwishClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SwishClientError::Swish(ref err) => write!(f, ": {}", err),
            SwishClientError::Http(ref err) => write!(f, ": {}", err),
            SwishClientError::Io(ref err) => write!(f, ": {}", err),
            SwishClientError::Json(ref err) => write!(f, ": {}", err),
            SwishClientError::Parse(ref err) => write!(f, ": {}", err),
            SwishClientError::Uri(ref err) => write!(f, ": {}", err),
            SwishClientError::ErrorCollection(ref err) => write!(
                f,
                ": {}",
                err.iter()
                    .fold(String::new(), |acc, curr| acc + &curr.to_string() + ", ")
            ),
        }
    }
}

impl From<RequestError> for SwishClientError {
    fn from(error: RequestError) -> SwishClientError {
        SwishClientError::Swish(error)
    }
}

impl From<ErrorCollection> for SwishClientError {
    fn from(error: ErrorCollection) -> SwishClientError {
        SwishClientError::ErrorCollection(error)
    }
}

impl From<hyper::Error> for SwishClientError {
    fn from(err: hyper::Error) -> SwishClientError {
        SwishClientError::Http(err)
    }
}

impl From<uri::InvalidUri> for SwishClientError {
    fn from(err: uri::InvalidUri) -> SwishClientError {
        SwishClientError::Uri(err)
    }
}

impl From<io::Error> for SwishClientError {
    fn from(err: io::Error) -> SwishClientError {
        SwishClientError::Io(err)
    }
}

impl From<serde_json::Error> for SwishClientError {
    fn from(err: serde_json::Error) -> SwishClientError {
        SwishClientError::Json(err)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub enum ErrorCode {
    // PayeePaymentReference is invalid.
    FF08,
    // Callback URL is missing or does not use Https.
    RP03,
    // Payer alias is invalid.
    BE18,
    // Payee alias is missing or empty.
    RP01,
    // Amount value is missing or not a valid number.
    PA02,
    // Amount value is too low.
    AM06,
    // Amount value is too large.
    AM02,
    // Invalid or missing Currency.
    AM03,
    // Wrong formatted message.
    RP02,
    // Another active PaymentRequest already exists for this payerAlias. Only applicable for E-Commerce.
    RP06,
    // Payer not Enrolled.
    ACMT03,
    // Counterpart is not activated.
    ACMT01,
    // Payee not Enrolled.
    ACMT07,
    // Parameter is not correct.
    PA01,
    // Original Payment not found or original payment is more than than 13 months old
    RF02,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct RequestError {
    #[serde(skip_deserializing)]
    pub http_status: hyper::StatusCode,

    #[serde(rename = "errorCode")]
    pub code: Option<ErrorCode>,

    #[serde(rename = "errorMessage")]
    pub message: String,

    #[serde(rename = "additionalInformation")]
    pub additional_information: Option<String>,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.http_status)
    }
}
