use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Markdown 消息构建器
#[derive(Debug, Clone, Default)]
pub struct MarkdownMessage {
    pub content: String,
    pub custom_template_id: Option<String>,
    pub params: Vec<MarkdownParam>,
}

impl MarkdownMessage {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            custom_template_id: None,
            params: Vec::new(),
        }
    }

    /// 使用自定义模板
    pub fn with_template(mut self, template_id: impl Into<String>) -> Self {
        self.custom_template_id = Some(template_id.into());
        self
    }

    /// 添加模板参数
    pub fn add_param(mut self, key: impl Into<String>, values: Vec<String>) -> Self {
        self.params.push(MarkdownParam {
            key: key.into(),
            values,
        });
        self
    }

    /// 转换为 API 格式
    pub fn to_value(&self) -> Value {
        if let Some(template_id) = &self.custom_template_id {
            json!({
                "custom_template_id": template_id,
                "params": self.params,
            })
        } else {
            json!({
                "content": self.content,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownParam {
    pub key: String,
    pub values: Vec<String>,
}

/// Embed 消息（富文本卡片）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Embed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<EmbedThumbnail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<EmbedField>>,
}

impl Embed {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    pub fn thumbnail(mut self, url: impl Into<String>) -> Self {
        self.thumbnail = Some(EmbedThumbnail { url: url.into() });
        self
    }

    pub fn add_field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        if self.fields.is_none() {
            self.fields = Some(Vec::new());
        }
        self.fields.as_mut().unwrap().push(EmbedField {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedThumbnail {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
}

/// Ark 消息（特殊卡片消息）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkMessage {
    pub template_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv: Option<Vec<ArkKv>>,
}

impl ArkMessage {
    pub fn new(template_id: i32) -> Self {
        Self {
            template_id,
            kv: None,
        }
    }

    pub fn add_kv(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if self.kv.is_none() {
            self.kv = Some(Vec::new());
        }
        self.kv.as_mut().unwrap().push(ArkKv {
            key: key.into(),
            value: value.into(),
            obj: None,
        });
        self
    }

    pub fn add_obj_kv(mut self, key: impl Into<String>, obj_kv: Vec<ArkObjKv>) -> Self {
        if self.kv.is_none() {
            self.kv = Some(Vec::new());
        }
        self.kv.as_mut().unwrap().push(ArkKv {
            key: key.into(),
            value: String::new(),
            obj: Some(obj_kv),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkKv {
    pub key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obj: Option<Vec<ArkObjKv>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkObjKv {
    pub obj_kv: Vec<ArkKvPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkKvPair {
    pub key: String,
    pub value: String,
}

/// Keyboard 消息按钮
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keyboard {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub content: KeyboardContent,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            id: None,
            content: KeyboardContent { rows: Vec::new() },
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn add_row(mut self, row: KeyboardRow) -> Self {
        self.content.rows.push(row);
        self
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardContent {
    pub rows: Vec<KeyboardRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardRow {
    pub buttons: Vec<Button>,
}

impl KeyboardRow {
    pub fn new() -> Self {
        Self {
            buttons: Vec::new(),
        }
    }

    pub fn add_button(mut self, button: Button) -> Self {
        self.buttons.push(button);
        self
    }
}

impl Default for KeyboardRow {
    fn default() -> Self {
        Self::new()
    }
}

/// 按钮
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Button {
    pub id: String,
    pub render_data: RenderData,
    pub action: ButtonAction,
}

impl Button {
    pub fn new(id: impl Into<String>, label: impl Into<String>, action_type: ActionType) -> Self {
        let label_string = label.into();
        Self {
            id: id.into(),
            render_data: RenderData {
                label: label_string.clone(),
                visited_label: label_string,
                style: ButtonStyle::Grey,
            },
            action: ButtonAction {
                action_type,
                permission: Permission::default(),
                data: String::new(),
                reply: false,
                enter: false,
                anchor: None,
            },
        }
    }

    pub fn with_style(mut self, style: ButtonStyle) -> Self {
        self.render_data.style = style;
        self
    }

    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.action.data = data.into();
        self
    }

    pub fn with_reply(mut self, reply: bool) -> Self {
        self.action.reply = reply;
        self
    }

    pub fn with_permission(mut self, permission: Permission) -> Self {
        self.action.permission = permission;
        self
    }

    pub fn with_link(mut self, url: impl Into<String>) -> Self {
        self.action.data = url.into();
        self.action.action_type = ActionType::Link;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderData {
    pub label: String,
    pub visited_label: String,
    pub style: ButtonStyle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ButtonStyle {
    #[serde(rename = "0")]
    Grey = 0,
    #[serde(rename = "1")]
    Blue = 1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonAction {
    #[serde(rename = "type")]
    pub action_type: ActionType,
    pub permission: Permission,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub data: String,
    #[serde(skip_serializing_if = "is_false")]
    pub reply: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub enter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ActionType {
    #[serde(rename = "0")]
    Link = 0,
    #[serde(rename = "1")]
    Callback = 1,
    #[serde(rename = "2")]
    AtBot = 2,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permission {
    #[serde(rename = "type")]
    pub permission_type: PermissionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specify_role_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specify_user_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum PermissionType {
    #[serde(rename = "0")]
    #[default]
    AllowSpecified = 0,
    #[serde(rename = "1")]
    Admin = 1,
    #[serde(rename = "2")]
    Everyone = 2,
    #[serde(rename = "3")]
    SpecifiedRole = 3,
}

/// 按钮交互事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub interaction_type: u32,
    pub application_id: String,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub user_openid: Option<String>,
    pub group_openid: Option<String>,
    pub data: InteractionData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionData {
    #[serde(rename = "type")]
    pub data_type: u32,
    pub resolved: Option<Value>,
}
