# Swish
[![Build Status](https://travis-ci.org/drager/swish.svg?branch=master)](https://travis-ci.org/drager/swish)
[![crates.io](https://img.shields.io/crates/v/swish-api.svg)](https://crates.io/crates/swish-api)
[![API docs](https://docs.rs/swish-api/badge.svg)](https://docs.rs/swish-api)
[![MIT/Apache-2.0 licensed](https://img.shields.io/crates/l/swish-api.svg)](https://github.com/drager/swish/tree/master/README.md#license)

Rust API bindings for the Swish API.
Built using [hyper](https://github.com/hyperium/hyper/) and [tokio](https://github.com/tokio-rs/tokio).

## Usage

A simple usage example:

```rust
extern crate swish;
extern crate tokio_core;

use swish::{client, error};
use tokio_core::reactor::Core;

fn main() {
    let core = Core::new().unwrap();
    let handle = core.handle();
    let current_dir = env::current_dir()?;
    let cert_path = current_dir.join("./test_cert.p12");
    let root_cert_path = current_dir.join("./root_cert.der");

    let swish_client = cert_path
        .into_os_string()
        .to_str()
        .and_then(|cert_path_string| {
            root_cert_path
                .into_os_string()
                .to_str()
                .map(|root_cert_path_string| {
                    client::SwishClient::new(
                        "1231181189",
                        cert_path_string,
                        root_cert_path_string,
                        "swish",
                        handle,
                    )
                })
        })
        .unwrap();


    let mut payment_params = client::PaymentParams::default();
    payment_params.amount = 100.00;
    payment_params.payee_alias = "1231181189";
    payment_params.payee_payment_reference = Some("0123456789");
    payment_params.callback_url = "https://example.com/api/swishcb/paymentrequests";
    payment_params.message = Some("Kingston USB Flash Drive 8 GB");

    let payment = client.create_payment(payment_params);
    let payment: Result<client::CreatedPayment, error::SwishClientError> = core.run(payment);
}
```

## Features and bugs

Please file feature requests and bugs at the [issue tracker][tracker].

[tracker]: https://github.com/drager/swish/issues

## License
Licensed under either of:

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

