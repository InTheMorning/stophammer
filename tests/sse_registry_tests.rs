//! SSE registry tests.

// ---------------------------------------------------------------------------
// Test: SSE registry can publish and receive events
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_publish_and_subscribe() {
    let registry = stophammer::api::SseRegistry::new();

    // Subscribe to artist-1
    let mut rx = registry
        .subscribe("artist-1")
        .expect("subscribe should succeed");

    // Publish an event for artist-1
    let frame = stophammer::api::SseFrame {
        event_type: "track_upserted".to_string(),
        subject_guid: "track-abc".to_string(),
        payload: serde_json::json!({"title": "New Song"}),
        seq: 1,
    };
    registry.publish("artist-1", frame.clone());

    // Should receive the event
    let received = rx.recv().await.expect("should receive event");
    assert_eq!(received.event_type, "track_upserted");
    assert_eq!(received.subject_guid, "track-abc");
}

// ---------------------------------------------------------------------------
// Test: SSE registry does not cross-pollinate between artists
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_no_cross_pollination() {
    let registry = stophammer::api::SseRegistry::new();

    let mut rx_a = registry
        .subscribe("artist-a")
        .expect("subscribe should succeed");
    let _rx_b = registry
        .subscribe("artist-b")
        .expect("subscribe should succeed");

    // Publish to artist-b only
    let frame = stophammer::api::SseFrame {
        event_type: "feed_upserted".to_string(),
        subject_guid: "feed-xyz".to_string(),
        payload: serde_json::json!({}),
        seq: 1,
    };
    registry.publish("artist-b", frame);

    // artist-a should NOT receive the event (try_recv should error)
    let result = rx_a.try_recv();
    assert!(
        result.is_err(),
        "artist-a should not receive artist-b events"
    );
}

// ---------------------------------------------------------------------------
// Test: SSE registry ring buffer stores recent events for replay
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_ring_buffer_replay() {
    let registry = stophammer::api::SseRegistry::new();

    // Publish 5 events before anyone subscribes
    for i in 0..5 {
        let frame = stophammer::api::SseFrame {
            event_type: "track_upserted".to_string(),
            subject_guid: format!("track-{i}"),
            payload: serde_json::json!({"n": i}),
            seq: i + 1,
        };
        registry.publish("artist-replay", frame);
    }

    // Get recent events for replay
    let recent = registry.recent_events("artist-replay");
    assert_eq!(recent.len(), 5, "should have 5 recent events");
    assert_eq!(recent[0].subject_guid, "track-0");
    assert_eq!(recent[4].subject_guid, "track-4");
}

// ---------------------------------------------------------------------------
// Test: Ring buffer is bounded to 100 events
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_ring_buffer_bounded() {
    let registry = stophammer::api::SseRegistry::new();

    // Publish 150 events
    for i in 0..150 {
        let frame = stophammer::api::SseFrame {
            event_type: "track_upserted".to_string(),
            subject_guid: format!("track-{i}"),
            payload: serde_json::json!({"n": i}),
            seq: i + 1,
        };
        registry.publish("artist-bounded", frame);
    }

    let recent = registry.recent_events("artist-bounded");
    assert_eq!(recent.len(), 100, "ring buffer must be bounded to 100");
    // Oldest event in buffer should be track-50 (first 50 were evicted)
    assert_eq!(recent[0].subject_guid, "track-50");
    assert_eq!(recent[99].subject_guid, "track-149");
}
