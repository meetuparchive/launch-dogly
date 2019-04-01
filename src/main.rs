//! A Launch Darkly [webhook handler](https://apidocs.launchdarkly.com/reference#webhooks-overview)
//! that records changes as Datadog events
use crypto::{
    hmac::Hmac,
    mac::{Mac, MacResult},
    sha2::Sha256,
};
use hex::FromHex;
use lambda_http::{lambda, IntoResponse, Request, RequestExt};
use lambda_runtime::{error::HandlerError, Context};
use reqwest::Client;
use serde_derive::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Env {
    ld_secret: String,
    dd_api_key: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Payload {
    accesses: Vec<Access>,
    kind: String,
    name: String,
    description: String,
    title_verb: String,
    title: String,
    member: Member,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Member {
    first_name: String,
    last_name: String,
}

#[derive(Deserialize, Debug)]
struct Access {
    action: String,
    resource: String,
}

fn main() {
    env_logger::init();
    lambda!(handler)
}

/// Record webhook as Datadog event
fn record(
    payload: Payload,
    dd_api_key: &str,
) {
    if payload.kind != "flag" {
        return;
    }

    // https://docs.datadoghq.com/api/?lang=python#post-an-event
    let event = json!({
        "title": format!(
            "{} {} {} {}",
            payload.member.first_name,
            payload.member.last_name,
            payload.title_verb,
            payload.name
        ),
         "text": payload.description,
         "tags": [
             format!("kind:{}", payload.kind),
             format!("name:{}", payload.name),
             format!("action:{}", payload.accesses[0].action)
         ],
         "source_type_name": "launch-darkly"
    });

    log::debug!("recording event {:#?}", event);
    if let Err(err) = Client::new()
        .post(&format!(
            "https://app.datadoghq.com/api/v1/events?api_key={}",
            dd_api_key
        ))
        .json(&event)
        .send()
    {
        eprintln!("failed to record event: {}", err)
    }
}

fn handler(
    request: Request,
    _: Context,
) -> Result<impl IntoResponse, HandlerError> {
    let Env {
        ld_secret,
        dd_api_key,
    } = envy::from_env::<Env>().map_err(|e| failure::err_msg(e.to_string()))?;

    if !authenticated(&request, &ld_secret) {
        eprintln!("request was not authenticated");
        return Ok(json!({
            "message": "Request not authenticated"
        }));
    }

    if let Ok(Some(payload)) = request.payload::<Payload>() {
        record(payload, &dd_api_key);
        return Ok(json!({
            "message": "👍"
        }));
    }

    Ok(json!({
        "message": "Failed to process request"
    }))
}

/// Verifies a request was triggered by ld
///
/// see [these docs](https://docs.launchdarkly.com/docs/webhooks#section-signing-webhooks) for
/// further reference
fn authenticated(
    request: &Request,
    secret: &str,
) -> bool {
    request
        .headers()
        .get("X-LD-Signature")
        .iter()
        .filter_map(|value| Vec::from_hex(value).ok())
        .any(|signature| {
            let mut mac = Hmac::new(Sha256::new(), &secret.as_bytes());
            mac.input(&request.body());
            mac.result() == MacResult::new(&signature)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_parses() {
        serde_json::from_str::<Payload>(include_str!("../tests/data/payload.json"))
            .expect("failed to parse payload");
    }

    #[test]
    fn authenticates_requests() {
        let body = include_str!("../tests/data/payload.json");

        let mut mac = Hmac::new(Sha256::new(), b"secret");
        mac.input(body.as_bytes());
        mac.result();
        let signature = hex::encode(mac.result().code());

        let request = http::Request::builder()
            .header("X-LD-Signature", signature)
            .body(body.into())
            .expect("failed to generate request");
        assert!(authenticated(&request, "secret"))
    }

    #[test]
    #[ignore]
    fn handler_handles() {
        let request = Request::default();
        let expected = json!({
        "message": "Go Serverless v1.0! Your function executed successfully!"
        })
        .into_response();
        let response = handler(request, Context::default())
            .expect("expected Ok(_) value")
            .into_response();
        assert_eq!(response.body(), expected.body())
    }
}
