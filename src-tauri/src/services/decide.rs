use serde_json::{json, Value};
use std::env;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::config::{DECISION_API_URL, DECISION_TIMEOUT_S, DEFAULT_DECISION_MODELS};
use crate::models::decision::Decision;

use super::failover;

pub const QUESTION_NAME: &str = "answer";

pub struct Decider {
    client: reqwest::Client,
    url: String,
    key: String,
    models: Vec<String>,
}

static SHARED: OnceLock<Decider> = OnceLock::new();

pub fn build_body(model: &str, text: &str, question: &str) -> Value {
    let mut que = serde_json::Map::new();
    que.insert(
        QUESTION_NAME.to_string(),
        json!({ "type": "noul", "instructions": question }),
    );
    json!({ "model": model, "state": text, "questions": que })
}

pub fn read_chance_of_yes(reply: &Value) -> Option<f32> {
    let chance = reply["answers"][QUESTION_NAME]["noul"].as_f64()? as f32;
    (0.0..=1.0).contains(&chance).then_some(chance)
}

impl Decider {
    pub fn new(url: String, key: String, models: Vec<String>, wait: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(wait)
            .build()
            .expect("could not create the HTTP client");
        Self {
            client,
            url,
            key,
            models,
        }
    }

    pub fn shared() -> &'static Decider {
        SHARED.get_or_init(Decider::from_env)
    }

    fn from_env() -> Self {
        let key = env::var("API_KEY").unwrap_or_default();
        let models = failover::model_list(
            env::var("AI_DECISION_MODEL").ok().as_deref(),
            &DEFAULT_DECISION_MODELS,
        );
        let wait = Duration::from_secs(DECISION_TIMEOUT_S);
        log::info!("Decider created (models: {models:?})");
        Self::new(DECISION_API_URL.to_string(), key, models, wait)
    }

    pub async fn yes_or_no(&self, text: &str, question: &str) -> Option<Decision> {
        for model in &self.models {
            if let Some(chances_yes) = self.ask_model(model, text, question).await {
                return Some(Decision::from_chance(chances_yes));
            }
        }
        None
    }

    pub async fn ask_model(&self, model: &str, text: &str, question: &str) -> Option<f32> {
        let started = Instant::now();

        let res = self
            .client
            .post(&self.url)
            .bearer_auth(&self.key)
            .json(&build_body(model, text, question))
            .send()
            .await
            .inspect_err(|e| log::warn!("decision model {model} did not answer: {e}"))
            .ok()?;

        let status = res.status();
        if !status.is_success() {
            log::warn!("decision model {model} answered with {status}");
            return None;
        }

        let reply: Value = res
            .json()
            .await
            .inspect_err(|e| log::warn!("decision model {model} sent a reply we cannot read: {e}"))
            .ok()?;

        let chance_of_yes = read_chance_of_yes(&reply);

        match chance_of_yes {
            Some(chance) => log::info!(
                "decision model {model}: {chance:.2} in {} ms",
                started.elapsed().as_millis()
            ),
            None => log::warn!("decision model {model} sent a reply with no usable answer"),
        }
        chance_of_yes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::decision::Verdict;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const QUESTION: &str = "Does the text contain a password?";

    fn reply_with_chance(chance: f64) -> String {
        json!({ "answers": { QUESTION_NAME: { "type": "noul", "noul": chance } } }).to_string()
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) {
        let mut seen = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let count = socket.read(&mut chunk).await.unwrap();
            if count == 0 {
                return;
            }
            seen.extend_from_slice(&chunk[..count]);
            let text = String::from_utf8_lossy(&seen).to_string();
            if let Some(head_end) = text.find("\r\n\r\n") {
                let announced = text[..head_end]
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::to_string)
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if seen.len() >= head_end + 4 + announced {
                    return;
                }
            }
        }
    }

    async fn fake_server(replies: Vec<(u16, Option<String>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/decisions", listener.local_addr().unwrap());
        tokio::spawn(async move {
            for (status, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_request(&mut socket).await;
                match body {
                    Some(body) => {
                        let answer = format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        socket.write_all(answer.as_bytes()).await.unwrap();
                    }
                    None => {
                        tokio::spawn(async move {
                            let _silent = socket;
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        });
                    }
                }
            }
        });
        url
    }

    fn decider_at(url: String, models: &[&str]) -> Decider {
        let models = models.iter().map(|m| m.to_string()).collect();
        Decider::new(
            url,
            "test-key".to_string(),
            models,
            Duration::from_millis(300),
        )
    }

    #[test]
    fn the_message_has_the_model_the_text_and_the_question() {
        let body = build_body("some/model", "my password is x", QUESTION);
        assert_eq!(body["model"], "some/model");
        assert_eq!(body["state"], "my password is x");
        assert_eq!(body["questions"][QUESTION_NAME]["type"], "noul");
        assert_eq!(body["questions"][QUESTION_NAME]["instructions"], QUESTION);
    }

    #[test]
    fn a_good_reply_gives_the_chance() {
        let reply: Value = serde_json::from_str(&reply_with_chance(0.97)).unwrap();
        let chance = read_chance_of_yes(&reply).unwrap();
        assert!((chance - 0.97).abs() < 1e-6);
    }

    #[test]
    fn odd_replies_give_nothing() {
        let odd = [
            json!({}),
            json!({ "answers": {} }),
            json!({ "answers": { QUESTION_NAME: { "type": "noul" } } }),
            json!({ "answers": { QUESTION_NAME: { "noul": "high" } } }),
            json!({ "answers": { QUESTION_NAME: { "noul": 1.5 } } }),
            json!({ "answers": { QUESTION_NAME: { "noul": -0.2 } } }),
            json!("just text"),
        ];
        for reply in odd {
            assert_eq!(read_chance_of_yes(&reply), None, "{reply}");
        }
    }

    #[tokio::test]
    async fn a_working_model_gives_a_decision() {
        let url = fake_server(vec![(200, Some(reply_with_chance(0.99)))]).await;
        let decision = decider_at(url, &["one"])
            .yes_or_no("text", QUESTION)
            .await
            .unwrap();
        assert_eq!(decision.verdict, Verdict::Yes);
    }

    #[tokio::test]
    async fn the_next_model_is_tried_when_the_first_fails() {
        let url = fake_server(vec![
            (500, Some("oops".into())),
            (200, Some(reply_with_chance(0.01))),
        ])
        .await;
        let decision = decider_at(url, &["broken", "working"])
            .yes_or_no("text", QUESTION)
            .await
            .unwrap();
        assert_eq!(decision.verdict, Verdict::No);
    }

    #[tokio::test]
    async fn a_model_that_stays_silent_is_given_up_on() {
        let url = fake_server(vec![(200, None), (200, Some(reply_with_chance(0.5)))]).await;
        let started = Instant::now();
        let decision = decider_at(url, &["silent", "working"])
            .yes_or_no("text", QUESTION)
            .await
            .unwrap();
        assert_eq!(decision.verdict, Verdict::Unsure);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_reply_we_cannot_use_counts_as_no_answer() {
        let url = fake_server(vec![(200, Some("not json".into()))]).await;
        assert_eq!(
            decider_at(url, &["one"]).yes_or_no("text", QUESTION).await,
            None
        );
    }

    #[tokio::test]
    async fn nothing_listening_gives_no_answer_and_no_crash() {
        let decider = decider_at("http://127.0.0.1:1/decisions".to_string(), &["one", "two"]);
        assert_eq!(decider.yes_or_no("text", QUESTION).await, None);
    }

    #[tokio::test]
    async fn no_models_gives_no_answer() {
        let decider = decider_at("http://127.0.0.1:1/decisions".to_string(), &[]);
        assert_eq!(decider.yes_or_no("text", QUESTION).await, None);
    }

    #[tokio::test]
    #[ignore]
    async fn the_real_models_answer() {
        dotenvy::dotenv().ok();
        let decider = Decider::shared();
        let secret = decider
            .yes_or_no("my password is hunter2", QUESTION)
            .await
            .expect("no decision model answered");
        let plain = decider
            .yes_or_no("Let's meet at 5pm tomorrow near the cafe", QUESTION)
            .await
            .expect("no decision model answered");
        println!("secret: {secret:?}\nplain: {plain:?}");
        assert_eq!(secret.verdict, Verdict::Yes);
        assert_eq!(plain.verdict, Verdict::No);
    }
}
