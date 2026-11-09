use std::{future::Future, pin::Pin};

use axum::http;
use openidconnect::{
    AsyncHttpClient, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config;

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// The identity provider configured for sign-in; it signs in invited users but never creates any.
pub struct Provider {
    pub name: String,
    issuer: IssuerUrl,
    client_id: ClientId,
    client_secret: ClientSecret,
    redirect: RedirectUrl,
    http: Http,
}

/// What the callback needs from the request that started the sign-in.
#[derive(Serialize, Deserialize)]
pub struct Pending {
    nonce: Nonce,
    pkce_verifier: PkceCodeVerifier,
}

pub struct Identity {
    pub issuer: String,
    pub subject: String,
}

#[derive(Debug, thiserror::Error)]
#[error("OIDC: {0}")]
pub struct Error(String);

fn error(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}

impl Provider {
    pub fn new(config: &config::Oidc, public_url: &Url) -> Result<Self, Error> {
        let redirect = public_url.join("api/oidc/callback").map_err(error)?;
        let http = reqwest::Client::builder()
            // following redirects would let the provider make the server request any URL
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(error)?;
        Ok(Self {
            name: config.name.clone(),
            issuer: IssuerUrl::from_url(config.issuer.clone()),
            client_id: ClientId::new(config.client_id.clone()),
            client_secret: ClientSecret::new(config.client_secret.clone()),
            redirect: RedirectUrl::from_url(redirect),
            http: Http(http),
        })
    }

    async fn client(&self) -> Result<(Client, CoreProviderMetadata), Error> {
        let metadata = CoreProviderMetadata::discover_async(self.issuer.clone(), &self.http)
            .await
            .map_err(error)?;
        let client = CoreClient::from_provider_metadata(
            metadata.clone(),
            self.client_id.clone(),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(self.redirect.clone());
        Ok((client, metadata))
    }

    /// Returns the provider's sign-in URL carrying `state`, and what the callback will need.
    pub async fn start(&self, state: String) -> Result<(Url, Pending), Error> {
        let (client, _) = self.client().await?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, _, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                || CsrfToken::new(state),
                Nonce::new_random,
            )
            .add_scope(Scope::new("openid".into()))
            .set_pkce_challenge(pkce_challenge)
            .url();
        Ok((
            url,
            Pending {
                nonce,
                pkce_verifier,
            },
        ))
    }

    pub async fn finish(&self, code: String, pending: Pending) -> Result<Identity, Error> {
        let (client, metadata) = self.client().await?;
        let response = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(error)?
            .set_pkce_verifier(pending.pkce_verifier)
            .request_async(&self.http)
            .await
            .map_err(error)?;
        let id_token = openidconnect::TokenResponse::id_token(&response)
            .ok_or_else(|| Error("no ID token".into()))?;
        let verifier = client
            .id_token_verifier()
            .set_allowed_algs(metadata.id_token_signing_alg_values_supported().clone());
        let claims = id_token.claims(&verifier, &pending.nonce).map_err(error)?;
        Ok(Identity {
            issuer: claims.issuer().to_string(),
            subject: claims.subject().to_string(),
        })
    }
}

struct Http(reqwest::Client);

impl<'c> AsyncHttpClient<'c> for Http {
    type Error = reqwest::Error;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(send(&self.0, request))
    }
}

async fn send(
    http: &reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, reqwest::Error> {
    let (parts, body) = request.into_parts();
    let response = http
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut out = http::Response::new(response.bytes().await?.to_vec());
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    Ok(out)
}
