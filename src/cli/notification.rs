use crate::api::schema::{Method, NotificationShowParams, NotificationShowSound, Request};
use crate::config::ToastHerdrPosition;

pub(super) fn run_notification_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_notification_help();
        return Ok(2);
    };

    match subcommand {
        "show" => notification_show(&args[1..]),
        "list" => super::print_response(&super::send_request(&Request {
            id: "cli:notification:list".into(),
            method: Method::NotificationList(Default::default()),
        })?),
        "clear" => super::print_response(&super::send_request(&Request {
            id: "cli:notification:clear".into(),
            method: Method::NotificationClear(Default::default()),
        })?),
        "help" | "--help" | "-h" => {
            print_notification_help();
            Ok(0)
        }
        _ => {
            print_notification_help();
            Ok(2)
        }
    }
}

fn notification_show(args: &[String]) -> std::io::Result<i32> {
    // An agent calling this from inside its own pane should not have to name
    // itself; the pane it runs in already says who it is.
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let params = match parse_notification_show_args(args, env_pane_id.as_deref()) {
        Ok(params) => params,
        Err(NotificationShowArgError::Usage) => {
            eprintln!(
                "usage: herdr notification show <title> [--body TEXT] [--pane PANE_ID] [--position top-left|top-right|bottom-left|bottom-right] [--sound none|done|request]"
            );
            return Ok(2);
        }
        Err(NotificationShowArgError::Message(message)) => {
            eprintln!("{message}");
            return Ok(2);
        }
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:notification:show".into(),
        method: Method::NotificationShow(params),
    })?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NotificationShowArgError {
    Usage,
    Message(String),
}

fn parse_notification_show_args(
    args: &[String],
    env_pane_id: Option<&str>,
) -> Result<NotificationShowParams, NotificationShowArgError> {
    let Some(title) = args.first().cloned() else {
        return Err(NotificationShowArgError::Usage);
    };
    if matches!(title.as_str(), "help" | "--help" | "-h") {
        return Err(NotificationShowArgError::Usage);
    }

    let mut body = None;
    let mut position = None;
    let mut sound = NotificationShowSound::None;
    let mut pane_id = env_pane_id.map(str::to_string);
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--body" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(NotificationShowArgError::Message(
                        "missing value for --body".into(),
                    ));
                };
                body = Some(value.clone());
                index += 2;
            }
            "--position" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(NotificationShowArgError::Message(
                        "missing value for --position".into(),
                    ));
                };
                position = Some(parse_toast_position(value)?);
                index += 2;
            }
            "--sound" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(NotificationShowArgError::Message(
                        "missing value for --sound".into(),
                    ));
                };
                sound = parse_notification_sound(value)?;
                index += 2;
            }
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(NotificationShowArgError::Message(
                        "missing value for --pane".into(),
                    ));
                };
                pane_id = Some(value.clone());
                index += 2;
            }
            other => {
                return Err(NotificationShowArgError::Message(format!(
                    "unknown option: {other}"
                )));
            }
        }
    }

    Ok(NotificationShowParams {
        title,
        body,
        pane_id: pane_id.map(|pane_id| super::normalize_pane_id(&pane_id)),
        position,
        sound,
    })
}

fn parse_toast_position(value: &str) -> Result<ToastHerdrPosition, NotificationShowArgError> {
    match value {
        "top-left" => Ok(ToastHerdrPosition::TopLeft),
        "top-right" => Ok(ToastHerdrPosition::TopRight),
        "bottom-left" => Ok(ToastHerdrPosition::BottomLeft),
        "bottom-right" => Ok(ToastHerdrPosition::BottomRight),
        _ => Err(NotificationShowArgError::Message(format!(
            "invalid position: {value} (expected top-left, top-right, bottom-left, or bottom-right)"
        ))),
    }
}

fn parse_notification_sound(
    value: &str,
) -> Result<NotificationShowSound, NotificationShowArgError> {
    match value {
        "none" => Ok(NotificationShowSound::None),
        "done" => Ok(NotificationShowSound::Done),
        "request" => Ok(NotificationShowSound::Request),
        _ => Err(NotificationShowArgError::Message(format!(
            "invalid sound: {value} (expected none, done, or request)"
        ))),
    }
}

fn print_notification_help() {
    eprintln!("herdr notification commands:");
    eprintln!(
        "  herdr notification show <title> [--body TEXT] [--pane PANE_ID] [--position top-left|top-right|bottom-left|bottom-right] [--sound none|done|request]"
    );
    eprintln!("  herdr notification list");
    eprintln!("  herdr notification clear");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn notification_show_args_parse_title_body_and_position() {
        let params = parse_notification_show_args(
            &args(&[
                "build failed",
                "--body",
                "api workspace",
                "--position",
                "top-right",
                "--sound",
                "request",
            ]),
            None,
        )
        .unwrap();

        assert_eq!(
            params,
            NotificationShowParams {
                title: "build failed".into(),
                body: Some("api workspace".into()),
                pane_id: None,
                position: Some(ToastHerdrPosition::TopRight),
                sound: NotificationShowSound::Request,
            }
        );
    }

    // An agent running inside a pane should be named by the notification it
    // raises without having to pass its own id.
    #[test]
    fn notification_show_args_take_the_calling_pane_from_the_environment() {
        let params = parse_notification_show_args(&args(&["build failed"]), Some("w1:p2")).unwrap();

        assert_eq!(params.pane_id.as_deref(), Some("w1:p2"));
    }

    #[test]
    fn explicit_pane_wins_over_the_environment() {
        let params = parse_notification_show_args(
            &args(&["build failed", "--pane", "w2:p9"]),
            Some("w1:p2"),
        )
        .unwrap();

        assert_eq!(params.pane_id.as_deref(), Some("w2:p9"));
    }

    #[test]
    fn notification_show_args_reject_missing_pane_value() {
        let error =
            parse_notification_show_args(&args(&["build failed", "--pane"]), None).unwrap_err();

        assert_eq!(
            error,
            NotificationShowArgError::Message("missing value for --pane".into())
        );
    }

    #[test]
    fn notification_show_args_reject_invalid_position() {
        let error = parse_notification_show_args(
            &args(&["build failed", "--position", "top-center"]),
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            NotificationShowArgError::Message(
                "invalid position: top-center (expected top-left, top-right, bottom-left, or bottom-right)"
                    .into()
            )
        );
    }

    #[test]
    fn notification_show_args_default_sound_is_none() {
        let params = parse_notification_show_args(&args(&["build failed"]), None).unwrap();

        assert_eq!(params.sound, NotificationShowSound::None);
    }

    #[test]
    fn notification_show_args_reject_invalid_sound() {
        let error = parse_notification_show_args(&args(&["build failed", "--sound", "loud"]), None)
            .unwrap_err();

        assert_eq!(
            error,
            NotificationShowArgError::Message(
                "invalid sound: loud (expected none, done, or request)".into()
            )
        );
    }
}
