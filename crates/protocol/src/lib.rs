pub mod agents {
    pub mod clusterscope {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/clusterscope.v1.rs"));
        }
    }
}

pub use agent_service_client::AgentServiceClient;
pub use agent_service_server::{AgentService, AgentServiceServer};
pub use agents::clusterscope::v1::*;
pub use central_service_client::CentralServiceClient;
pub use central_service_server::{CentralService, CentralServiceServer};

// Re-export tonic types
pub use tonic;
