use corgigram_core::turn::fetch_elixir_webrtc_turn;
use corgigram_transport::{run_answerer_role, run_offerer_role, IceConfig};

#[tokio::test]
async fn elixir_turn_trickle_connect() {
    let mut ice = IceConfig::default();
    let turn = fetch_elixir_webrtc_turn("corgigram-test")
        .await
        .expect("fetch elixir-webrtc turn");
    ice.add_turn_server(turn);

    let (offerer, offer_sdp) = run_offerer_role(&ice).await.expect("offerer");
    let (answerer, answer_sdp) = run_answerer_role(&ice, &offer_sdp)
        .await
        .expect("answerer");
    offerer
        .apply_remote_answer(&answer_sdp)
        .await
        .expect("apply answer");

    for i in 0..300 {
        for c in offerer.drain_local_candidates().await {
            let _ = answerer.add_remote_candidate(&c).await;
        }
        for c in answerer.drain_local_candidates().await {
            let _ = offerer.add_remote_candidate(&c).await;
        }
        if offerer.is_connected() && answerer.is_connected() {
            println!("connected after {i} iterations");
            offerer.wait_ready().await.expect("offerer ready");
            answerer.wait_ready().await.expect("answerer ready");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("peers did not connect within timeout");
}
