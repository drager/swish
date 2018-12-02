extern crate swish_api;
extern crate tokio_core;

use std::env;
use std::{thread, time};
use swish_api::{client, error};
use tokio_core::reactor::Core;

fn get_client_and_core() -> Result<(client::SwishClient, Core), error::SwishClientError> {
    let core = tokio_core::reactor::Core::new().unwrap();
    let handle = core.handle();
    let current_dir = env::current_dir()?;
    let cert_path = current_dir.join("./tests/test_cert.p12");
    let swish_client = cert_path
        .into_os_string()
        .to_str()
        .map(|cert_path_string| {
            client::SwishClient::new("1231181189", cert_path_string, "swish", handle)
        }).unwrap();

    Ok((swish_client, core))
}

fn get_default_params<'a>() -> client::PaymentParams<'a> {
    let mut payment_params = client::PaymentParams::default();
    payment_params.amount = 100.00;
    payment_params.payee_alias = "1231181189";
    payment_params.payee_payment_reference = Some("0123456789");
    payment_params.callback_url = "https://example.com/api/swishcb/paymentrequests";
    payment_params.message = Some("Kingston USB Flash Drive 8 GB");
    payment_params
}

#[test]
fn test_create_payment_ecommerce() {
    let (client, mut core) = get_client_and_core().unwrap();
    let mut payment_params = get_default_params();
    payment_params.payer_alias = Some("46712345678");

    let payment = client.create_payment(payment_params);
    let payment: Result<client::CreatedPayment, error::SwishClientError> = core.run(payment);

    assert!(payment.is_ok());
    let ok_payment = payment.unwrap();
    assert_eq!(ok_payment.id.is_empty(), false);
    assert_eq!(ok_payment.location.is_empty(), false);
    assert!(ok_payment.request_token.is_none());
}

#[test]
fn test_create_payment_mcommerce() {
    let (client, mut core) = get_client_and_core().unwrap();
    let payment_params = get_default_params();

    let payment = client.create_payment(payment_params);
    let payment: Result<client::CreatedPayment, error::SwishClientError> = core.run(payment);

    assert!(payment.is_ok());
    let ok_payment = payment.unwrap();
    assert_eq!(ok_payment.id.is_empty(), false);
    assert_eq!(ok_payment.location.is_empty(), false);
    assert!(ok_payment.request_token.is_some());
}

#[test]
#[ignore]
fn test_create_payment_error_for_callback_error() {
    let (client, mut core) = get_client_and_core().unwrap();
    let mut payment_params = get_default_params();
    payment_params.callback_url = "http://example.com/api/swishcb/paymentrequests";

    let payment = client.create_payment(payment_params);
    let payment: Result<client::CreatedPayment, error::SwishClientError> = core.run(payment);

    assert_eq!(format!("{}", payment.unwrap_err()), "a");
    // panic!(payment.unwrap_err().error_message);
    // assert!(payment.unwrap_err());
}

#[test]
fn test_get_payment() {
    let (client, mut core) = get_client_and_core().unwrap();
    let payment_params = get_default_params();

    let created_payment = client.create_payment(payment_params);
    let created_payment: Result<client::CreatedPayment, error::SwishClientError> =
        core.run(created_payment);

    let payment: Result<client::Payment, error::SwishClientError> = created_payment
        .and_then(|created_payment| core.run(client.get_payment(created_payment.id.as_str())));

    assert!(payment.is_ok());
    let ok_payment = payment.unwrap();
    assert_eq!(ok_payment.id.is_empty(), false);
    assert_eq!(ok_payment.amount, 100.00);
    assert_eq!(ok_payment.currency, client::Currency::SEK);
    assert!(ok_payment.status.is_some());
    let ok_status = ok_payment.status.unwrap();
    assert_eq!(ok_status, client::Status::Created);
    assert_eq!(ok_payment.date_created.is_empty(), false);
    assert!(ok_payment.message.is_some());
    let ok_message = ok_payment.message.unwrap();
    assert_eq!(ok_message, "Kingston USB Flash Drive 8 GB");
    assert!(ok_payment.payee_alias.is_some());
    assert!(ok_payment.payee_payment_reference.is_some());
}

#[test]
fn test_create_refund() {
    let (client, mut core) = get_client_and_core().unwrap();
    let payment_params = get_default_params();

    let created_payment = client.create_payment(payment_params);
    let created_payment: Result<client::CreatedPayment, error::SwishClientError> =
        core.run(created_payment);

    // We need too wait five seconds so the payment has been created.
    let five_seconds = time::Duration::from_millis(5000);
    thread::sleep(five_seconds);

    let refund: Result<client::CreatedRefund, error::SwishClientError> = created_payment
        .and_then(|created_payment| core.run(client.get_payment(created_payment.id.as_str())))
        .and_then(|gotten_payment| {
            let payment_reference = gotten_payment.payment_reference.unwrap();
            let mut refund_params = client::RefundParams::default();
            refund_params.amount = 100.00;
            refund_params.callback_url = "https://example.com/api/swishcb/refunds";
            refund_params.original_payment_reference = payment_reference.as_str();
            refund_params.payer_payment_reference = Some("0123456789");
            refund_params.message = Some("Refund for Kingston USB Flash Drive 8 GB");

            let refund = client.create_refund(refund_params);
            let refund = core.run(refund);
            refund
        });

    assert!(refund.is_ok());
    let ok_refund = refund.unwrap();
    assert_eq!(ok_refund.id.is_empty(), false);
    assert_eq!(ok_refund.location.is_empty(), false);
}

#[test]
fn test_get_refund() {
    let (client, mut core) = get_client_and_core().unwrap();
    let payment_params = get_default_params();

    let created_payment = client.create_payment(payment_params);
    let created_payment: Result<client::CreatedPayment, error::SwishClientError> =
        core.run(created_payment);

    // We need too wait five seconds so the payment has been created.
    let five_seconds = time::Duration::from_millis(5000);
    thread::sleep(five_seconds);

    let created_refund: Result<client::CreatedRefund, error::SwishClientError> = created_payment
        .and_then(|created_payment| core.run(client.get_payment(created_payment.id.as_str())))
        .and_then(|gotten_payment| {
            let payment_reference = gotten_payment.payment_reference.unwrap();
            let mut refund_params = client::RefundParams::default();
            refund_params.amount = 100.00;
            refund_params.callback_url = "https://example.com/api/swishcb/refunds";
            refund_params.original_payment_reference = payment_reference.as_str();
            refund_params.payer_payment_reference = Some("0123456789");
            refund_params.message = Some("Refund for Kingston USB Flash Drive 8 GB");

            let refund = client.create_refund(refund_params);
            let refund = core.run(refund);
            refund
        });

    let gotten_refund = created_refund
        .and_then(|created_refund| core.run(client.get_refund(created_refund.id.as_str())));

    assert!(gotten_refund.is_ok());
    let ok_refund = gotten_refund.unwrap();
    assert_eq!(ok_refund.id.is_empty(), false);
    assert_eq!(ok_refund.amount, 100.00);
    assert_eq!(ok_refund.currency, client::Currency::SEK);
    assert!(ok_refund.status.is_some());
    let ok_status = ok_refund.status.unwrap();
    assert_eq!(ok_status, client::Status::Initiated);
    assert_eq!(ok_refund.date_created.is_empty(), false);
    assert!(ok_refund.message.is_some());
    let ok_message = ok_refund.message.unwrap();
    assert_eq!(ok_message, "Refund for Kingston USB Flash Drive 8 GB");
    assert!(ok_refund.payer_payment_reference.is_some());
}
