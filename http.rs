use crate::consts::TIMEOUT;

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).build()
}
