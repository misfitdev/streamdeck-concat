use std::sync::Mutex;

use openaction::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::concat::{self, Status};
use crate::keys::{self, Command};

/// `keys::send`, with the failure shown on the key or dial.
async fn press(instance: &Instance, command: Command, times: u32) -> OpenActionResult<()> {
    if let Err(error) = keys::send(command, times).await {
        log::warn!("{command:?}: {error:?}");
        instance.show_alert().await?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct CommandSettings {
    command: Command,
}

impl Default for CommandSettings {
    fn default() -> Self {
        CommandSettings {
            command: Command::Play,
        }
    }
}

pub struct CommandKey;

#[async_trait]
impl Action for CommandKey {
    const UUID: ActionUuid = "dev.misfit.concat.command";
    type Settings = CommandSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        instance
            .set_title(Some(settings.command.label()), None)
            .await
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        instance
            .set_title(Some(settings.command.label()), None)
            .await
    }

    async fn key_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        press(instance, settings.command, 1).await
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct JogSettings {
    coarse: bool,
    push: Command,
}

impl Default for JogSettings {
    fn default() -> Self {
        JogSettings {
            coarse: false,
            push: Command::Play,
        }
    }
}

pub struct JogDial;

impl JogDial {
    async fn render(instance: &Instance, settings: &JogSettings) -> OpenActionResult<()> {
        let step = if settings.coarse {
            "10 frames"
        } else {
            "1 frame"
        };
        instance
            .set_feedback(&json!({ "title": "Jog", "value": step }))
            .await
    }
}

#[async_trait]
impl Action for JogDial {
    const UUID: ActionUuid = "dev.misfit.concat.jog";
    type Settings = JogSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        Self::render(instance, settings).await
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        Self::render(instance, settings).await
    }

    async fn dial_rotate(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        ticks: i16,
        pressed: bool,
    ) -> OpenActionResult<()> {
        // Turning while held is the coarse step without changing mode.
        let coarse = settings.coarse != pressed;
        let command = match (ticks > 0, coarse) {
            (true, false) => Command::StepForward,
            (false, false) => Command::StepBack,
            (true, true) => Command::StepForward10,
            (false, true) => Command::StepBack10,
        };
        press(instance, command, ticks.unsigned_abs().into()).await
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        press(instance, settings.push, 1).await
    }

    async fn touch_tap(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        _position: (u16, u16),
        _hold: bool,
    ) -> OpenActionResult<()> {
        let next = JogSettings {
            coarse: !settings.coarse,
            ..settings.clone()
        };
        instance.set_settings(&next).await?;
        Self::render(instance, &next).await
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ZoomSettings {
    push: Command,
}

impl Default for ZoomSettings {
    fn default() -> Self {
        ZoomSettings {
            push: Command::Split,
        }
    }
}

pub struct ZoomDial;

#[async_trait]
impl Action for ZoomDial {
    const UUID: ActionUuid = "dev.misfit.concat.zoom";
    type Settings = ZoomSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let push = settings.push.label().replace('\n', " ");
        instance
            .set_feedback(&json!({ "title": "Zoom", "value": push }))
            .await
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.will_appear(instance, settings).await
    }

    async fn dial_rotate(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
        ticks: i16,
        _pressed: bool,
    ) -> OpenActionResult<()> {
        let command = if ticks > 0 {
            Command::ZoomIn
        } else {
            Command::ZoomOut
        };
        press(instance, command, ticks.unsigned_abs().into()).await
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        press(instance, settings.push, 1).await
    }
}

pub struct StatusKey;

impl StatusKey {
    fn title(status: &Status) -> String {
        match status {
            Status::Offline => "Concat\nOffline".into(),
            Status::Unauthorized => "Concat\nNo token".into(),
            Status::Online { version } => format!("Concat\n{version}"),
        }
    }

    /// Keeps every visible status key in step with the connection.
    pub async fn watch() {
        let mut status = concat::client().status();
        loop {
            let title = Self::title(&status.borrow_and_update());
            for instance in visible_instances(Self::UUID).await {
                let _ = instance.set_title(Some(title.clone()), None).await;
            }
            if status.changed().await.is_err() {
                return;
            }
        }
    }
}

#[async_trait]
impl Action for StatusKey {
    const UUID: ActionUuid = "dev.misfit.concat.status";
    type Settings = Value;

    async fn will_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let title = Self::title(&concat::client().status().borrow());
        instance.set_title(Some(title), None).await
    }

    async fn key_down(
        &self,
        _instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        keys::activate().await;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct RenderSettings {
    /// A Concat project folder. Must not be open in the window: the API
    /// refuses projects the window holds.
    project: String,
    /// The file to write; ~/Movies/<project>.mp4 when empty.
    output: String,
}

struct Job {
    /// None until export.run answers; its events can arrive first, and
    /// only one export runs at a time, so any event then is this job's.
    id: Option<String>,
    instance: InstanceId,
    path: String,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

pub struct RenderAction;

impl RenderAction {
    async fn show(instance: &Instance, label: &str, percent: Option<u8>) -> OpenActionResult<()> {
        if instance.controller == "Encoder" {
            let value = percent.map_or(label.to_owned(), |p| format!("{p}%"));
            instance
                .set_feedback(&json!({
                    "title": "Render",
                    "value": value,
                    "indicator": { "value": percent.unwrap_or(0) },
                }))
                .await
        } else {
            let title = percent.map_or(label.to_owned(), |p| format!("Render\n{p}%"));
            instance.set_title(Some(title), None).await
        }
    }

    async fn start(instance: &Instance, settings: &RenderSettings) -> OpenActionResult<()> {
        let path = settings.project.trim().to_owned();
        if path.is_empty() {
            instance.show_alert().await?;
            return Self::show(instance, "No project", None).await;
        }
        let output = if settings.output.trim().is_empty() {
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "export".into());
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/Movies/{name}.mp4")
        } else {
            settings.output.trim().to_owned()
        };

        let claimed = {
            let mut slot = JOB.lock().unwrap();
            let free = slot.is_none();
            if free {
                *slot = Some(Job {
                    id: None,
                    instance: instance.instance_id.clone(),
                    path: path.clone(),
                });
            }
            free
        };
        if !claimed {
            return instance.show_alert().await;
        }
        let client = concat::client();
        if let Err(error) = client.call("project.open", json!({ "path": path })).await {
            log::warn!("project.open {path}: {} {}", error.code, error.message);
            *JOB.lock().unwrap() = None;
            instance.show_alert().await?;
            let label = if error.code == "refused" {
                "In use"
            } else {
                "Failed"
            };
            return Self::show(instance, label, None).await;
        }
        match client
            .call("export.run", json!({ "path": path, "output": output }))
            .await
        {
            Ok(started) => {
                let id = started["job"].as_str().unwrap_or_default().to_owned();
                let pending = match JOB.lock().unwrap().as_mut() {
                    Some(job) if job.id.is_none() => {
                        job.id = Some(id);
                        true
                    }
                    _ => false,
                };
                if pending {
                    Self::show(instance, "", Some(0)).await?;
                }
                Ok(())
            }
            Err(error) => {
                log::warn!("export.run {path}: {} {}", error.code, error.message);
                *JOB.lock().unwrap() = None;
                let _ = client
                    .call("project.close", json!({ "path": path, "save": false }))
                    .await;
                instance.show_alert().await?;
                let label = if error.code == "busy" {
                    "Busy"
                } else {
                    "Failed"
                };
                Self::show(instance, label, None).await
            }
        }
    }

    /// Follows export events for the job this plugin started.
    pub async fn watch() {
        let mut events = concat::client().events();
        let mut status = concat::client().status();
        loop {
            let event = tokio::select! {
                received = events.recv() => match received {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                },
                changed = status.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if matches!(*status.borrow_and_update(), Status::Online { .. }) {
                        continue;
                    }
                    // The connection's API session, and the job with it, is gone.
                    let Some(job) = JOB.lock().unwrap().take() else {
                        continue;
                    };
                    if let Some(instance) = get_instance(job.instance).await {
                        let _ = instance.show_alert().await;
                        let _ = Self::show(&instance, "Lost", None).await;
                    }
                    continue;
                }
            };
            let job_id = event.params["job"].as_str().unwrap_or_default();
            let (instance_id, path, finished) = {
                let mut slot = JOB.lock().unwrap();
                let Some(job) = slot
                    .as_ref()
                    .filter(|job| job.id.as_deref().is_none_or(|id| id == job_id))
                else {
                    continue;
                };
                let finished = matches!(event.method.as_str(), "export.done" | "export.failed");
                let found = (job.instance.clone(), job.path.clone(), finished);
                if finished {
                    *slot = None;
                }
                found
            };
            let Some(instance) = get_instance(instance_id).await else {
                continue;
            };
            let _ = match event.method.as_str() {
                "export.progress" => {
                    let frame = event.params["frame"].as_f64().unwrap_or(0.0);
                    let total = event.params["total"].as_f64().unwrap_or(0.0).max(1.0);
                    let percent = (frame / total * 100.0).clamp(0.0, 100.0) as u8;
                    Self::show(&instance, "", Some(percent)).await
                }
                "export.done" => {
                    let _ = instance.show_ok().await;
                    Self::show(&instance, "Done", None).await
                }
                "export.failed" => {
                    let code = event.params["error"]["code"].as_str().unwrap_or_default();
                    if code != "cancelled" {
                        let _ = instance.show_alert().await;
                    }
                    let label = if code == "cancelled" {
                        "Cancelled"
                    } else {
                        "Failed"
                    };
                    Self::show(&instance, label, None).await
                }
                _ => Ok(()),
            };
            if finished {
                let _ = concat::client()
                    .call("project.close", json!({ "path": path, "save": false }))
                    .await;
            }
        }
    }

    async fn toggle(instance: &Instance, settings: &RenderSettings) -> OpenActionResult<()> {
        let running = JOB
            .lock()
            .unwrap()
            .as_ref()
            .map(|job| (job.id.clone(), job.instance == instance.instance_id));
        match running {
            Some((None, true)) => Ok(()),
            Some((Some(id), true)) => {
                let _ = concat::client()
                    .call("export.cancel", json!({ "job": id }))
                    .await;
                Ok(())
            }
            Some((_, false)) => instance.show_alert().await,
            None => Self::start(instance, settings).await,
        }
    }
}

#[async_trait]
impl Action for RenderAction {
    const UUID: ActionUuid = "dev.misfit.concat.render";
    type Settings = RenderSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        Self::show(instance, "Render", None).await
    }

    async fn key_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        Self::toggle(instance, settings).await
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        Self::toggle(instance, settings).await
    }

    async fn touch_tap(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        _position: (u16, u16),
        _hold: bool,
    ) -> OpenActionResult<()> {
        Self::toggle(instance, settings).await
    }
}
