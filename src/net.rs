//! Shared blocking HTTP agent. One lightweight agent, reused everywhere.

use std::time::Duration;

use ureq::Agent;

pub const USER_AGENT: &str = concat!("mewsic/", env!("CARGO_PKG_VERSION"));

pub fn agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(12))
            .user_agent(USER_AGENT)
            .build()
    })
}
