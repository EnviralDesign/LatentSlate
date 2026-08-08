from pathlib import Path


path = Path("src/providers/latentslate_engine.rs")
text = path.read_text().replace("\r\n", "\n")

old_loop = '''    let mut job: EngineJob =
        parse_json_response(response, "LatentSlate Engine job submission").await?;

    loop {
        if let Some(progress) = job.progress {
            if let Some(tx) = progress_tx.as_ref() {
                let _ = tx.send(ProviderProgress::overall(progress.clamp(0.0, 1.0) as f32));
            }
        }
        match job.status.as_str() {
            "queued" | "running" => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let response = send_with_auth(
                    client.get(endpoint(base_url, &format!("/v1/jobs/{}", job.id))),
                    api_key,
                )
                .send()
                .await
                .map_err(|err| offline("LatentSlate Engine job polling", err))?;
                job = parse_json_response(response, "LatentSlate Engine job polling").await?;
            }
'''
new_loop = '''    let mut job: EngineJob =
        parse_json_response(response, "LatentSlate Engine job submission").await?;
    let mut unchanged_polls = 0_u32;

    loop {
        if let Some(progress) = job.progress {
            if let Some(tx) = progress_tx.as_ref() {
                let _ = tx.send(ProviderProgress::overall(progress.clamp(0.0, 1.0) as f32));
            }
        }
        match job.status.as_str() {
            "queued" | "running" => {
                tokio::time::sleep(engine_poll_delay(unchanged_polls)).await;
                let response = send_with_auth(
                    client.get(endpoint(base_url, &format!("/v1/jobs/{}", job.id))),
                    api_key,
                )
                .send()
                .await
                .map_err(|err| offline("LatentSlate Engine job polling", err))?;
                let next_job: EngineJob =
                    parse_json_response(response, "LatentSlate Engine job polling").await?;
                if engine_job_poll_changed(&job, &next_job) {
                    unchanged_polls = 0;
                } else {
                    unchanged_polls = unchanged_polls.saturating_add(1);
                }
                job = next_job;
            }
'''
if text.count(old_loop) != 1:
    raise SystemExit(f"Expected one Engine polling loop, found {text.count(old_loop)}")
text = text.replace(old_loop, new_loop, 1)

anchor = '''fn content_type_for_path(path: &Path) -> Option<&'static str> {
'''
helpers = '''fn engine_poll_delay(unchanged_polls: u32) -> Duration {
    match unchanged_polls {
        0..=3 => Duration::from_millis(350),
        4..=9 => Duration::from_secs(1),
        _ => Duration::from_secs(2),
    }
}

fn engine_job_poll_changed(previous: &EngineJob, next: &EngineJob) -> bool {
    let progress_changed = match (previous.progress, next.progress) {
        (Some(previous), Some(next)) => (previous - next).abs() > 0.000_001,
        (None, None) => false,
        _ => true,
    };
    previous.status != next.status || previous.message != next.message || progress_changed
}

fn content_type_for_path(path: &Path) -> Option<&'static str> {
'''
if text.count(anchor) != 1:
    raise SystemExit(f"Expected one content-type helper anchor, found {text.count(anchor)}")
text = text.replace(anchor, helpers, 1)

test_anchor = '''    #[test]
    fn catalog_tools_normalize_into_provider_entries() {
'''
tests = '''    fn test_engine_job(
        status: &str,
        progress: Option<f64>,
        message: Option<&str>,
    ) -> EngineJob {
        EngineJob {
            id: Uuid::nil(),
            status: status.to_string(),
            progress,
            message: message.map(str::to_string),
            artifacts: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn engine_poll_delay_is_responsive_then_backs_off() {
        assert_eq!(engine_poll_delay(0), Duration::from_millis(350));
        assert_eq!(engine_poll_delay(3), Duration::from_millis(350));
        assert_eq!(engine_poll_delay(4), Duration::from_secs(1));
        assert_eq!(engine_poll_delay(9), Duration::from_secs(1));
        assert_eq!(engine_poll_delay(10), Duration::from_secs(2));
        assert_eq!(engine_poll_delay(u32::MAX), Duration::from_secs(2));
    }

    #[test]
    fn engine_job_poll_change_detects_meaningful_updates() {
        let base = test_engine_job("running", Some(0.25), Some("Generating"));
        assert!(!engine_job_poll_changed(
            &base,
            &test_engine_job("running", Some(0.25), Some("Generating")),
        ));
        assert!(engine_job_poll_changed(
            &base,
            &test_engine_job("running", Some(0.5), Some("Generating")),
        ));
        assert!(engine_job_poll_changed(
            &base,
            &test_engine_job("running", Some(0.25), Some("Encoding")),
        ));
        assert!(engine_job_poll_changed(
            &base,
            &test_engine_job("succeeded", Some(1.0), Some("Complete")),
        ));
    }

    #[test]
    fn catalog_tools_normalize_into_provider_entries() {
'''
if text.count(test_anchor) != 1:
    raise SystemExit(f"Expected one first Engine adapter test anchor, found {text.count(test_anchor)}")
text = text.replace(test_anchor, tests, 1)

path.write_text(text)
