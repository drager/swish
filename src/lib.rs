extern crate hyper;
extern crate native_tls;
extern crate tokio_core;
extern crate hyper_tls;
extern crate serde;
extern crate futures;

#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

pub mod client;
pub mod error;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}