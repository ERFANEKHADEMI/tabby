use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;

use super::OAuthClient;
use crate::schema::auth::{AuthenticationService, OAuthCredential, OAuthProvider};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GithubOAuthResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    token_type: String,

    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
    #[serde(default)]
    error_uri: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GithubUserEmail {
    email: String,
    primary: bool,
    verified: bool,
    visibility: Option<String>,
}

pub struct GithubClient {
    client: reqwest::Client,
    auth: Arc<dyn AuthenticationService>,
}

impl GithubClient {
    pub fn new(auth: Arc<dyn AuthenticationService>) -> Self {
        Self {
            client: reqwest::Client::new(),
            auth,
        }
    }

    async fn read_credential(&self) -> Result<OAuthCredential> {
        match self
            .auth
            .read_oauth_credential(OAuthProvider::Github)
            .await?
        {
            Some(credential) => Ok(credential),
            None => Err(anyhow::anyhow!("No Github OAuth credential found")),
        }
    }

    async fn exchange_access_token(
        &self,
        code: String,
        credential: OAuthCredential,
    ) -> Result<GithubOAuthResponse> {
        let params = [
            ("client_id", credential.client_id.as_str()),
            ("code", code.as_str()),
        ];
        let resp = self
            .client
            .post("https://github.com/login/oauth/access_token")
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&params)
            .send()
            .await?
            .json::<GithubOAuthResponse>()
            .await?;

        Ok(resp)
    }
}

#[async_trait]
impl OAuthClient for GithubClient {
    async fn fetch_user_email(&self, code: String) -> Result<String> {
        let credentials = self.read_credential().await?;
        let token_resp = self.exchange_access_token(code, credentials).await?;
        if !token_resp.error.is_empty() {
            return Err(anyhow::anyhow!(
                "Failed to exchange access token: {}",
                token_resp.error_description
            ));
        }

        let resp = self
            .client
            .get("https://api.github.com/user/emails")
            .header(reqwest::header::USER_AGENT, "Tabby")
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", token_resp.access_token),
            )
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;

        let emails = resp.json::<Vec<GithubUserEmail>>().await?;

        for item in &emails {
            if item.primary {
                return Ok(item.email.clone());
            }
        }

        return Err(anyhow::anyhow!("No primary email address found"));
    }

    async fn get_authorization_url(&self) -> Result<String> {
        let credentials = self.read_credential().await?;
        let mut url = reqwest::Url::parse("https://github.com/login/oauth/authorize")?;
        let params = vec![
            ("client_id", credentials.client_id.as_str()),
            ("response_type", "code"),
            ("scope", "read:user user:email"),
        ];
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }
        Ok(url.to_string())
    }
}
