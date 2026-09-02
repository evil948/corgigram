use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use corgigram_crypto::{Identity, PreKeyBundle, SessionInitiator, SessionResponder};
use corgigram_protocol::WireMessage;
use corgigram_transport::{
    create_answer, create_offer, run_answerer_role, run_local_demo, run_offerer_role, IceConfig,
};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "corgigram", about = "E2E messenger connectivity test")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate and save local identity (includes private keys — keep safe)
    Identity {
        #[arg(long, default_value = "user")]
        user_id: String,
        #[arg(long, default_value = "User")]
        name: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Export public pre-key bundle for sharing with contacts
    ExportBundle {
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Create WebRTC offer SDP
    Offer {
        #[arg(long)]
        out: PathBuf,
    },
    /// Create WebRTC answer SDP from offer
    Answer {
        #[arg(long)]
        offer: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Offerer: write offer, wait for answer file, send encrypted message
    Ping {
        #[arg(long)]
        out_offer: PathBuf,
        #[arg(long)]
        wait_answer: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        remote_bundle: PathBuf,
        #[arg(long, default_value = "encrypted ping")]
        message: String,
    },
    /// Answerer: wait for offer file, write answer, receive encrypted message
    Pong {
        #[arg(long)]
        wait_offer: PathBuf,
        #[arg(long)]
        out_answer: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        remote_bundle: PathBuf,
    },
    /// Full local demo: WebRTC + E2E encrypted message (single machine)
    Demo {
        #[arg(long, default_value = "Привет! E2E работает.")]
        message: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("webrtc=warn".parse()?))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Identity { user_id, name, out } => cmd_identity(user_id, name, out),
        Commands::ExportBundle { identity, out } => cmd_export_bundle(identity, out),
        Commands::Offer { out } => cmd_offer(out).await,
        Commands::Answer { offer, out } => cmd_answer(offer, out).await,
        Commands::Ping {
            out_offer,
            wait_answer,
            identity,
            remote_bundle,
            message,
        } => cmd_ping(out_offer, wait_answer, identity, remote_bundle, message).await,
        Commands::Pong {
            wait_offer,
            out_answer,
            identity,
            remote_bundle,
        } => cmd_pong(wait_offer, out_answer, identity, remote_bundle).await,
        Commands::Demo { message } => cmd_demo(message).await,
    }
}

fn cmd_identity(user_id: String, name: String, out: PathBuf) -> Result<()> {
    let identity = Identity::generate(user_id, name);
    let json = identity.save_json()?;
    fs::write(&out, json).with_context(|| format!("write {}", out.display()))?;
    println!("identity written to {} (keep this file private)", out.display());
    Ok(())
}

fn cmd_export_bundle(identity_path: PathBuf, out: PathBuf) -> Result<()> {
    let identity = load_identity(identity_path)?;
    let bundle = identity.prekey_bundle();
    let json = serde_json::to_string_pretty(&bundle)?;
    fs::write(&out, json).with_context(|| format!("write {}", out.display()))?;
    println!("public bundle written to {}", out.display());
    Ok(())
}

async fn cmd_offer(out: PathBuf) -> Result<()> {
    let ice = IceConfig::default();
    let offer_sdp = create_offer(&ice).await?;
    fs::write(&out, &offer_sdp).with_context(|| format!("write {}", out.display()))?;
    println!("offer written to {}", out.display());
    Ok(())
}

async fn cmd_answer(offer_path: PathBuf, out: PathBuf) -> Result<()> {
    let offer_sdp = fs::read_to_string(&offer_path)
        .with_context(|| format!("read {}", offer_path.display()))?;
    let ice = IceConfig::default();
    let answer_sdp = create_answer(&ice, &offer_sdp).await?;
    fs::write(&out, &answer_sdp).with_context(|| format!("write {}", out.display()))?;
    println!("answer written to {}", out.display());
    Ok(())
}

async fn cmd_ping(
    out_offer: PathBuf,
    wait_answer: PathBuf,
    identity_path: PathBuf,
    remote_bundle_path: PathBuf,
    message: String,
) -> Result<()> {
    let identity = load_identity(identity_path)?;
    let remote_bundle = load_bundle(remote_bundle_path)?;
    let ice = IceConfig::localhost();

    let (mut peer, offer_sdp) = run_offerer_role(&ice, true).await?;
    fs::write(&out_offer, &offer_sdp).with_context(|| format!("write {}", out_offer.display()))?;
    println!("offer written to {}, waiting for answer...", out_offer.display());

    let answer_sdp = wait_for_file(&wait_answer, 120).await?;
    peer.set_remote_answer(&answer_sdp).await?;

    let (initiator, init) = SessionInitiator::begin(identity, &remote_bundle)?;
    peer.send(&WireMessage::SessionInit(init).to_bytes()?).await?;

    let ack_bytes = recv_with_timeout(&mut peer, 30).await?;
    let ack = match WireMessage::from_bytes(&ack_bytes)? {
        WireMessage::SessionAck(ack) => ack,
        other => anyhow::bail!("expected session ack, got {:?}", other),
    };

    let mut session = initiator.complete(&ack)?;
    let encrypted = session.encrypt(message.as_bytes())?;
    peer.send(&WireMessage::EncryptedChat { ciphertext: encrypted }.to_bytes()?)
        .await?;

    println!("sent encrypted message to {}", session.remote().display_name);
    Ok(())
}

async fn cmd_pong(
    wait_offer: PathBuf,
    out_answer: PathBuf,
    identity_path: PathBuf,
    remote_bundle_path: PathBuf,
) -> Result<()> {
    let identity = load_identity(identity_path)?;
    let remote_bundle = load_bundle(remote_bundle_path)?;
    let ice = IceConfig::localhost();

    println!("waiting for offer at {}...", wait_offer.display());
    let offer_sdp = wait_for_file(&wait_offer, 120).await?;

    let (mut peer, answer_sdp) = run_answerer_role(&ice, &offer_sdp, true).await?;
    fs::write(&out_answer, &answer_sdp)
        .with_context(|| format!("write {}", out_answer.display()))?;
    println!("answer written to {}", out_answer.display());

    peer.wait_ready().await?;

    let init_bytes = recv_with_timeout(&mut peer, 30).await?;
    let init = match WireMessage::from_bytes(&init_bytes)? {
        WireMessage::SessionInit(init) => init,
        other => anyhow::bail!("expected session init, got {:?}", other),
    };

    let (responder, ack) = SessionResponder::accept(identity, &init)?;
    peer.send(&WireMessage::SessionAck(ack).to_bytes()?).await?;

    let mut session = responder.complete(&init)?;

    let chat_bytes = recv_with_timeout(&mut peer, 30).await?;
    let ciphertext = match WireMessage::from_bytes(&chat_bytes)? {
        WireMessage::EncryptedChat { ciphertext } => ciphertext,
        other => anyhow::bail!("expected encrypted chat, got {:?}", other),
    };

    let plaintext = session.decrypt(&ciphertext)?;
    println!(
        "received from {}: {}",
        session.remote().display_name,
        String::from_utf8_lossy(&plaintext)
    );
    println!("safety number: {}", session.local().safety_number(session.remote()));
    let _ = remote_bundle;
    Ok(())
}

async fn wait_for_file(path: &PathBuf, timeout_secs: u64) -> Result<String> {
    for _ in 0..timeout_secs * 2 {
        if path.exists() {
            return Ok(fs::read_to_string(path)?);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    anyhow::bail!("timed out waiting for {}", path.display())
}

async fn recv_with_timeout(
    peer: &mut corgigram_transport::PeerConnection,
    secs: u64,
) -> Result<Vec<u8>> {
    tokio::time::timeout(std::time::Duration::from_secs(secs), peer.recv())
        .await
        .context("timed out waiting for message")?
        .context("connection closed")
}

fn load_bundle(path: PathBuf) -> Result<PreKeyBundle> {
    let json = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&json)?)
}

fn load_identity(path: PathBuf) -> Result<Identity> {
    let json = fs::read_to_string(&path)?;
    Ok(Identity::load_json(&json)?)
}

async fn cmd_demo(message: String) -> Result<()> {
    println!("=== korki demo: crypto + WebRTC + E2E message ===\n");

    let alice = Identity::generate("alice", "Alice");
    let bob = Identity::generate("bob", "Bob");
    let bob_bundle = bob.prekey_bundle();
    let alice_bundle = alice.prekey_bundle();

    println!("[1/4] Identities created");
    println!("      Alice safety w/ Bob: {}", alice.public.safety_number(&bob.public));
    assert!(bob_bundle.verify().is_ok());
    assert!(alice_bundle.verify().is_ok());
    println!("      Pre-key bundles verified\n");

    println!("[2/4] WebRTC connecting (local host candidates)...");
    let ice = IceConfig::localhost();
    let (mut alice_peer, mut bob_peer) = run_local_demo(&ice).await?;
    println!("      WebRTC connected\n");

    println!("[3/4] E2E session handshake...");
    let (initiator, init) = SessionInitiator::begin(alice, &bob_bundle)?;
    alice_peer.send(&WireMessage::SessionInit(init).to_bytes()?).await?;

    let init_bytes = recv_with_timeout(&mut bob_peer, 30).await?;
    let init = match WireMessage::from_bytes(&init_bytes)? {
        WireMessage::SessionInit(init) => init,
        other => anyhow::bail!("expected session init, got {:?}", other),
    };

    let (responder, ack) = SessionResponder::accept(bob, &init)?;
    bob_peer.send(&WireMessage::SessionAck(ack).to_bytes()?).await?;

    let ack_bytes = recv_with_timeout(&mut alice_peer, 30).await?;
    let ack = match WireMessage::from_bytes(&ack_bytes)? {
        WireMessage::SessionAck(ack) => ack,
        other => anyhow::bail!("expected session ack, got {:?}", other),
    };

    let mut alice_session = initiator.complete(&ack)?;
    let mut bob_session = responder.complete(&init)?;
    println!("      E2E session established\n");

    println!("[4/4] Sending encrypted message...");
    let encrypted = alice_session.encrypt(message.as_bytes())?;
    alice_peer
        .send(&WireMessage::EncryptedChat { ciphertext: encrypted }.to_bytes()?)
        .await?;

    let chat_bytes = recv_with_timeout(&mut bob_peer, 30).await?;
    let ciphertext = match WireMessage::from_bytes(&chat_bytes)? {
        WireMessage::EncryptedChat { ciphertext } => ciphertext,
        other => anyhow::bail!("expected encrypted chat, got {:?}", other),
    };

    let plaintext = bob_session.decrypt(&ciphertext)?;
    let received = String::from_utf8_lossy(&plaintext);

    println!("\n=== SUCCESS ===");
    println!("Bob received: \"{received}\"");
    println!("Safety number: {}", bob_session.local().safety_number(bob_session.remote()));
    Ok(())
}

