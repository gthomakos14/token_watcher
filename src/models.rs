use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub name: String,
    pub email: String,
    pub profile_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuota {
    pub name: String,
    pub id: u32,
    pub remaining_percentage: f32,
    pub used_percentage: f32,
    pub reset_timestamp: Option<i64>,
    pub reset_time: Option<String>,
    pub time_until_reset: Option<String>,
    pub badge: Option<String>,
    pub is_selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenReport {
    pub user: Option<UserInfo>,
    pub plan: Option<PlanInfo>,
    pub selected_model_id: Option<u32>,
    pub selected_model_name: Option<String>,
    pub models: Vec<ModelQuota>,
    pub fetched_at: i64,
}
