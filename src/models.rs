use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrintRequest {
    pub(crate) text: Option<String>,
    pub(crate) items: Option<Vec<String>>,
    pub(crate) image: Option<String>,
    pub(crate) image_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsUpdate {
    pub(crate) printer_name: String,
    pub(crate) paper_width: u16,
}
