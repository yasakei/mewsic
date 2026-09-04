use std::time::Duration;

use ureq::Agent;

pub const USER_AGENT: &str = concat!("mewsic/", env!("CARGO_PKG_VERSION"));

fn build_agent(timeout: Duration) -> Agent {
    ureq::AgentBuilder::new()
        .timeout(timeout)
        .user_agent(USER_AGENT)
        .build()
}

pub fn discord_agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| build_agent(Duration::from_secs(5)))
}

pub fn spotify_agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| build_agent(Duration::from_secs(5)))
}

pub fn lyrics_agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| build_agent(Duration::from_secs(8)))
}

pub fn lastfm_agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| build_agent(Duration::from_secs(8)))
}

pub fn local_agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| build_agent(Duration::from_secs(2)))
}
