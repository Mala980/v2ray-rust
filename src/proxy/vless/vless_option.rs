use crate::proxy::Address;
use uuid::Uuid;

#[derive(Clone)]
pub struct VlessOption {
    pub uuid: Uuid,
    pub addr: Address,
    pub is_udp: bool,
    pub flow: Option<String>,
}
