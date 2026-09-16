//! Read-only Linux desktop appearance discovery. Only the normal GUI host starts
//! this service: restricted script workers never connect to the session bus.

use mg_chassis::ColorScheme;
use std::{
    sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    thread,
    time::Duration,
};
use zbus::{
    Address,
    address::Transport,
    blocking::{Connection, connection::Builder},
    zvariant::OwnedValue,
};

const INTERVAL: Duration = Duration::from_secs(2);
const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.Settings";

/// Return immediately, then supply the desktop scheme from one background worker.
///
/// The standardized Settings portal is polled every two seconds. Missing buses,
/// unsupported portals, unknown preferences and errors resolve to light. A
/// one-slot channel prevents a stalled window from accumulating notifications;
/// each poll retries the current value and notices when the receiver is dropped.
/// No desktop settings are changed and no helper executable is launched.
pub fn spawn_watcher() -> Receiver<ColorScheme> {
    let (sender, receiver) = sync_channel(1);
    // Failure to start this optional service leaves the host's light fallback.
    let _ = thread::Builder::new()
        .name("mg-appearance".into())
        .spawn(move || {
            let mut connection = None;
            loop {
                if connection.as_ref().is_some_and(Connection::is_closed) {
                    connection = None;
                }
                if connection.is_none() {
                    connection = Address::session().and_then(connect).ok();
                }
                let scheme = connection
                    .as_ref()
                    .and_then(|connection| read_preference(connection).ok())
                    .unwrap_or_default();
                if !publish(&sender, scheme) {
                    break;
                }
                thread::sleep(INTERVAL);
            }
            if let Some(connection) = connection {
                let _ = connection.close();
            }
        });
    receiver
}

fn publish(sender: &SyncSender<ColorScheme>, scheme: ColorScheme) -> bool {
    match sender.try_send(scheme) {
        Ok(()) | Err(TrySendError::Full(_)) => true,
        Err(TrySendError::Disconnected(_)) => false,
    }
}

fn connect(address: Address) -> zbus::Result<Connection> {
    // Theme discovery needs only the local session socket. In particular, do not
    // let a unixexec/autolaunch address turn this into a helper-process launcher.
    if !matches!(address.transport(), Transport::Unix(_)) {
        return Err(zbus::Error::Unsupported);
    }
    Builder::address(address)?
        .method_timeout(INTERVAL)
        .max_queued(8)
        .build()
}

fn read_preference(connection: &Connection) -> zbus::Result<ColorScheme> {
    let read = |method| {
        connection.call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            method,
            &("org.freedesktop.appearance", "color-scheme"),
        )
    };
    let reply = match read("ReadOne") {
        // ReadOne was added in Settings v2. Older Read has an extra variant
        // layer; do not retry arbitrary failures or malformed modern replies.
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod" =>
        {
            read("Read")?
        }
        result => result?,
    };
    Ok(decode(&reply.body().deserialize::<OwnedValue>()?))
}

fn decode(value: &OwnedValue) -> ColorScheme {
    // downcast_ref accepts the normal u32 or one legacy nested variant, while
    // rejecting strings, integers of another width and further nested values.
    match value.downcast_ref::<u32>() {
        Ok(1) => ColorScheme::Dark,
        _ => ColorScheme::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    #[test]
    fn standardized_portal_values_and_unknowns() {
        for (value, expected) in [
            (0, ColorScheme::Light),
            (1, ColorScheme::Dark),
            (2, ColorScheme::Light),
            (3, ColorScheme::Light),
            (u32::MAX, ColorScheme::Light),
        ] {
            assert_eq!(decode(&OwnedValue::from(value)), expected);
        }
    }

    #[test]
    fn legacy_nested_variant_is_accepted_but_wrong_types_are_light() {
        let legacy = OwnedValue::try_from(Value::Value(Box::new(Value::U32(1)))).unwrap();
        assert_eq!(decode(&legacy), ColorScheme::Dark);
        for value in [
            Value::I32(1),
            Value::U64(1),
            Value::from("dark"),
            Value::Bool(true),
            Value::Value(Box::new(Value::Value(Box::new(Value::U32(1))))),
        ] {
            assert_eq!(
                decode(&OwnedValue::try_from(value).unwrap()),
                ColorScheme::Light
            );
        }
    }

    #[test]
    fn notifications_are_bounded_nonblocking_and_stop_after_receiver_drops() {
        let (sender, receiver) = sync_channel(1);
        assert!(publish(&sender, ColorScheme::Dark));
        for _ in 0..1000 {
            assert!(publish(&sender, ColorScheme::Light));
        }
        assert_eq!(receiver.try_recv().unwrap(), ColorScheme::Dark);
        assert!(receiver.try_recv().is_err());
        assert!(publish(&sender, ColorScheme::Light));
        assert_eq!(receiver.try_recv().unwrap(), ColorScheme::Light);
        drop(receiver);
        assert!(!publish(&sender, ColorScheme::Dark));
    }

    #[test]
    fn missing_socket_and_nonlocal_bus_addresses_fail_without_helpers() {
        for address in [
            "unix:path=/nonexistent/mgbrowser-theme-test/bus",
            "tcp:host=127.0.0.1,port=1",
            "unixexec:path=/nonexistent/mgbrowser-theme-test/helper",
        ] {
            assert!(connect(address.try_into().unwrap()).is_err());
        }
    }
}
