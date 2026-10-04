//! Sends the "how to install the app" email via AWS SES, once, right after
//! a brand-new user's first Google sign-in. Every call site treats this the
//! same way `audit::log` is already treated elsewhere in this crate: best
//! effort, errors only logged, never allowed to fail the request that
//! triggered it — a user should never see a 500 because SES hiccuped, is
//! still in sandbox mode, or the EC2 instance role hasn't been attached yet.

use aws_sdk_sesv2::{
    types::{Body, Content, Destination, EmailContent, Message},
    Client,
};

/// Fixed to the one verified sending identity this project has — not worth
/// a config entry since changing it means re-verifying a domain in SES
/// anyway, a manual console step, not a deploy-time toggle.
const SENDER_EMAIL: &str = "noreply@taktikdansiasat.com";

pub async fn send_welcome_email(
    client: &Client,
    install_url: &str,
    to_email: &str,
    to_name: &str,
) -> Result<(), aws_sdk_sesv2::Error> {
    let subject = Content::builder()
        .data("Selamat datang di Taktik dan Siasat")
        .charset("UTF-8")
        .build()
        .expect("subject is a non-empty string literal");

    let body_text = format!(
        "Halo {to_name},\n\
        \n\
        Terima kasih sudah mendaftar di Taktik dan Siasat!\n\
        \n\
        Untuk mulai memakai aplikasinya:\n\
        1. Install aplikasi gratis \"Expo Go\" dari App Store atau Play Store.\n\
        2. Buka tautan berikut dari HP kamu:\n\
        \n\
        {install_url}\n\
        \n\
        Sampai jumpa di jalur!"
    );

    let body = Body::builder()
        .text(
            Content::builder()
                .data(body_text)
                .charset("UTF-8")
                .build()
                .expect("body is a non-empty string"),
        )
        .build();

    client
        .send_email()
        .from_email_address(SENDER_EMAIL)
        .destination(Destination::builder().to_addresses(to_email).build())
        .content(
            EmailContent::builder()
                .simple(Message::builder().subject(subject).body(body).build())
                .build(),
        )
        .send()
        .await?;

    Ok(())
}
