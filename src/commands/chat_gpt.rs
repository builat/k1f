async fn ask_openai(prompt: String, api_key: &str) -> anyhow::Result<String> {
    let client = Client::new();
    let request = OpenAIRequest {
        model: "gpt-4.1".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            input: prompt,
        }],
    };

    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await?
        .json::<OpenAIResponse>()
        .await?;

    Ok(res
        .choices
        .get(0)
        .map(|c| c.message.content.clone())
        .unwrap_or_default())
}

pub struct AskGpt<'cr> {
    pub chat_request: &'cr ChatRequest,
}
