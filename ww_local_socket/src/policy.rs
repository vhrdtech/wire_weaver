//! Who may connect to a listening socket (SRV-46): the kernel's file permissions keep everyone else away from the
//! socket, and every accepted connection is checked again against the peer's credentials.

use std::fmt;

/// Who may connect to a socket made by [listen_with](crate::listen_with) or taken by [inherited](crate::inherited),
/// and the modes of the socket file and its directory ([dir_with](crate::dir_with)) that go with it.
///
/// | Policy      | Socket file           | Directory             | Accepted                                      |
/// |-------------|-----------------------|-----------------------|-----------------------------------------------|
/// | `SameUser`  | 0600                  | 0700                  | this process's user                           |
/// | `Users`     | 0660, group `group`   | 0710, group `group`   | this user, `uids` (members of `group` if empty) |
/// | `Any`       | 0666                  | 0711                  | everyone, the identity passed to the handlers |
///
/// The peer's identity is the kernel's word (`SO_PEERCRED`, `getpeereid`), never what the client says about itself.
/// Group membership, supplementary groups included, is checked by the kernel on the socket file: the peer
/// credentials carry only the effective group. A connection of another user is dropped before a byte is read.
/// Windows has `SameUser` only (the pipe's DACL and the token user).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PeerPolicy {
    /// Only processes of this process's user: today's rule and the default.
    #[default]
    SameUser,
    /// This user and the users the socket's group admits, narrowed to `uids` when that is not empty. Others are
    /// kept out by the kernel (they can't open the socket file) and refused again at accept.
    Users {
        /// The group of the socket file and its directory: its members reach the socket. This process's user
        /// must be a member to set it.
        group: u32,
        /// Users accepted besides this one; empty: every member of `group`.
        uids: Vec<u32>,
    },
    /// Every local user: the socket is world-writable, and who called is up to the server's authorization
    /// (`wire_weaver_server::Authorize`), which sees each caller's uid.
    Any,
}

/// Why [PeerPolicy] refused a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The peer runs as a user the policy doesn't take.
    User { uid: u32, me: u32 },
    /// The OS doesn't say who the peer is.
    Unknown,
    /// The socket file or its directory lets in more than the policy does: an inherited socket whose
    /// `SocketMode=` or `SocketGroup=` doesn't match, or an abstract socket, which has no permissions at all.
    Loose(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::User { uid, me } => {
                write!(
                    f,
                    "the other side runs as uid {uid}, which the policy of uid {me} doesn't take"
                )
            }
            Refused::Unknown => f.write_str("the OS doesn't tell who is on the other side"),
            Refused::Loose(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Refused {}

impl PeerPolicy {
    /// The socket file's mode.
    pub fn socket_mode(&self) -> u32 {
        match self {
            PeerPolicy::SameUser => 0o600,
            PeerPolicy::Users { .. } => 0o660,
            PeerPolicy::Any => 0o666,
        }
    }

    /// The mode of the directory the socket is in: search only for the others, never listing or writing.
    pub fn dir_mode(&self) -> u32 {
        match self {
            PeerPolicy::SameUser => 0o700,
            PeerPolicy::Users { .. } => 0o710,
            PeerPolicy::Any => 0o711,
        }
    }

    /// The group of the socket file and its directory, `None` to leave the creator's.
    pub fn group(&self) -> Option<u32> {
        match self {
            PeerPolicy::Users { group, .. } => Some(*group),
            PeerPolicy::SameUser | PeerPolicy::Any => None,
        }
    }

    /// Whether a peer that runs as `uid` is taken by a listener of user `me`. `None`: the OS didn't say.
    ///
    /// # Errors
    ///
    /// [Refused::User] or [Refused::Unknown].
    pub fn admits(&self, me: u32, uid: Option<u32>) -> Result<(), Refused> {
        if *self == PeerPolicy::Any {
            return Ok(());
        }
        let uid = uid.ok_or(Refused::Unknown)?;
        let admitted = match self {
            PeerPolicy::SameUser => uid == me,
            // the kernel admitted only members of `group` (and root) through the socket file's mode
            PeerPolicy::Users { uids, .. } => uid == me || uids.is_empty() || uids.contains(&uid),
            PeerPolicy::Any => true,
        };
        if admitted {
            Ok(())
        } else {
            Err(Refused::User { uid, me })
        }
    }

    /// Whether a socket file or directory with `mode` and group `gid` lets in no one the policy doesn't: the
    /// check of an inherited socket, whose file the service manager made. `what` names it in the error.
    ///
    /// # Errors
    ///
    /// [Refused::Loose].
    pub fn check_mode(&self, what: &str, mode: u32, gid: u32) -> Result<(), Refused> {
        let loose = |why: String| Err(Refused::Loose(format!("{what}: {why}")));
        match self {
            PeerPolicy::SameUser if mode & 0o077 != 0 => loose(format!(
                "mode {:o} lets in other users, the policy is the same user only",
                mode & 0o777
            )),
            PeerPolicy::Users { .. } if mode & 0o007 != 0 => loose(format!(
                "mode {:o} lets in every user, the policy is a group",
                mode & 0o777
            )),
            PeerPolicy::Users { group, .. } if gid != *group => {
                loose(format!("group {gid}, the policy's is {group}"))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 1000;

    #[test]
    fn same_user_takes_only_this_user() {
        let policy = PeerPolicy::default();
        assert_eq!(policy, PeerPolicy::SameUser);
        assert_eq!(policy.admits(ME, Some(ME)), Ok(()));
        assert_eq!(
            policy.admits(ME, Some(1001)),
            Err(Refused::User { uid: 1001, me: ME })
        );
        assert_eq!(
            policy.admits(ME, Some(0)),
            Err(Refused::User { uid: 0, me: ME })
        );
        assert_eq!(policy.admits(ME, None), Err(Refused::Unknown));
    }

    #[test]
    fn users_takes_the_list_and_this_user() {
        let policy = PeerPolicy::Users {
            group: 50,
            uids: vec![1001, 1002],
        };
        assert_eq!(policy.admits(ME, Some(ME)), Ok(()));
        assert_eq!(policy.admits(ME, Some(1002)), Ok(()));
        assert_eq!(
            policy.admits(ME, Some(1003)),
            Err(Refused::User { uid: 1003, me: ME })
        );
        assert_eq!(
            policy.admits(ME, Some(0)),
            Err(Refused::User { uid: 0, me: ME })
        );
        assert_eq!(policy.admits(ME, None), Err(Refused::Unknown));
    }

    #[test]
    fn users_without_a_list_takes_whom_the_group_lets_in() {
        let policy = PeerPolicy::Users {
            group: 50,
            uids: vec![],
        };
        assert_eq!(policy.admits(ME, Some(1003)), Ok(()));
        assert_eq!(policy.admits(ME, None), Err(Refused::Unknown));
    }

    #[test]
    fn any_takes_everyone() {
        assert_eq!(PeerPolicy::Any.admits(ME, Some(1003)), Ok(()));
        assert_eq!(PeerPolicy::Any.admits(ME, None), Ok(()));
    }

    #[test]
    fn modes_follow_the_policy() {
        let users = PeerPolicy::Users {
            group: 50,
            uids: vec![],
        };
        let modes = |p: &PeerPolicy| (p.socket_mode(), p.dir_mode(), p.group());
        assert_eq!(modes(&PeerPolicy::SameUser), (0o600, 0o700, None));
        assert_eq!(modes(&users), (0o660, 0o710, Some(50)));
        assert_eq!(modes(&PeerPolicy::Any), (0o666, 0o711, None));
        // nothing is world-writable but Any
        for p in [PeerPolicy::SameUser, users] {
            assert_eq!(p.socket_mode() & 0o007, 0, "{p:?}");
            assert_eq!(p.dir_mode() & 0o006, 0, "{p:?}");
        }
    }

    #[test]
    fn loose_files_are_refused() {
        let same = PeerPolicy::SameUser;
        assert!(same.check_mode("s", 0o600, 7).is_ok());
        assert!(same.check_mode("s", 0o700, 7).is_ok());
        assert!(matches!(
            same.check_mode("s", 0o660, 7),
            Err(Refused::Loose(_))
        ));
        assert!(matches!(
            same.check_mode("s", 0o604, 7),
            Err(Refused::Loose(_))
        ));
        let users = PeerPolicy::Users {
            group: 50,
            uids: vec![],
        };
        assert!(users.check_mode("s", 0o660, 50).is_ok());
        assert!(matches!(
            users.check_mode("s", 0o660, 51),
            Err(Refused::Loose(_))
        ));
        assert!(matches!(
            users.check_mode("s", 0o666, 50),
            Err(Refused::Loose(_))
        ));
        assert!(PeerPolicy::Any.check_mode("s", 0o777, 1).is_ok());
    }
}
