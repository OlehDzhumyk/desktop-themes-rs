// SPDX-License-Identifier: GPL-3.0-or-later
//! Who may read the clipboard history and drive the mixer (ADR 0008). The check is shared with
//! the theme service (`themes --dbus-service`) and lives in [`desk_proto::trust`].

pub use desk_proto::trust::*;
