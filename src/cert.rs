use std::time::Duration;

use rcgen::{Certificate, CertificateParams, DistinguishedName};
use tokio::time::sleep;
use tracing::{error, info};

use acme::{
    Account, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    OrderStatus,
};


// Create a new account. This will generate a fresh ECDSA key for you.
// Alternatively, restore an account from serialized credentials by
// using `Account::from_credentials()`.
async fn account() -> Account {
    let account = Account::create(
        &NewAccount {
            contact: &[],
            terms_of_service_agreed: true,
            only_return_existing: false,
        },
        LetsEncrypt::Staging.url(),
    )
    .await?;
}

// Create the ACME order based on the given domain names.
// Note that this only needs an `&Account`, so the library will let you
// process multiple orders in parallel for a single account.
fn order(account: &Account, names: &[String]) -> () {
    let identifiers = names
        .iter()
        .map(|name| Identifier::Dns(name.into()))
        .collect::<Vec<_>>();
    let (order, state) = account
        .new_order(&NewOrder {
            identifiers: &identifiers,
        })
        .await
        .unwrap();

    info!("order state: {:#?}", state);
    (order, state);
}

// Pick the desired challenge type and prepare the response.
fn prepare_response(order: (), state: ()) -> () {
    let authorizations = order.authorizations(&state.authorizations).await.unwrap();
    let mut challenges = Vec::with_capacity(authorizations.len());
    for authz in &authorizations {
        match authz.status {
            AuthorizationStatus::Pending => {}
            AuthorizationStatus::Valid => continue,
            _ => todo!(),
        }

        // We'll use the DNS challenges for this example, but you could
        // pick something else to use here.
        let challenge = authz
            .challenges
            .iter()
            .find(|c| c.r#type == ChallengeType::Dns01)
            .ok_or_else(|| anyhow::anyhow!("no dns01 challenge found"))?;

        let Identifier::Dns(identifier) = &authz.identifier;
        // TODO: set up the challenge response here.
        challenges.push((identifier, &challenge.url));
    }
}

fn setup_response_server() -> () {
    todo!()
}

// Exponentially back off until the order becomes ready or invalid.
fn wait_for_order_rdy(order: ()) {
    let mut tries = 1u8;
    let mut delay = Duration::from_millis(250);
    let state = loop {
        sleep(delay).await;
        let state = order.state().await.unwrap();
        if let OrderStatus::Ready | OrderStatus::Invalid = state.status {
            info!("order state: {:#?}", state);
            break state;
        }

        delay *= 2;
        tries += 1;
        match tries < 5 {
            true => info!(?state, tries, "order is not ready, waiting {delay:?}"),
            false => {
                error!(?state, tries, "order is not ready");
                return Err(anyhow::anyhow!("order is not ready"));
            }
        }
    };
}

// If the order is ready, we can provision the certificate.
// Use the rcgen library to create a Certificate Signing Request.
fn prepare_sign_request() -> () {
    let mut params = CertificateParams::new(names.clone());
    params.distinguished_name = DistinguishedName::new();
    let cert = Certificate::from_params(params).unwrap();
    cert.serialize_request_der()?
}

async fn provision(names: Vec<String>, port: u16) -> anyhow::Result<()> {
    let account = account();
    let (mut order, state) = order(&account, &names);

    prepare_response(order, state);
    setup_response_server(port);

    // Let the server know we're ready to accept the challenges.
    for (_, url) in &challenges {
        order.set_challenge_ready(url).await.unwrap();
    }

    let state = wait_for_order_rdy(order);
    if state.status == OrderStatus::Invalid {
        return Err(anyhow::anyhow!("order is invalid"));
    }

    let mut names = Vec::with_capacity(challenges.len());
    for (identifier, _) in challenges {
        names.push(identifier.to_owned());
    }

    let csr = prepare_sign_request();

    // Finalize the order and print certificate chain, private key and account credentials.
    let cert_chain_pem = order.finalize(&csr, &state.finalize).await.unwrap();
    info!("certficate chain:\n\n{}", cert_chain_pem,);
    info!("private key:\n\n{}", cert.serialize_private_key_pem());
    info!(
        "account credentials:\n\n{}",
        serde_json::to_string_pretty(&account.credentials()).unwrap()
    );

    Ok(())
}
