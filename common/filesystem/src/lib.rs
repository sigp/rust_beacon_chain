use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    /// The file could not be created
    UnableToCreateFile(io::Error),
    /// The file could not be opened
    UnableToOpenFile(io::Error),
    /// Failed to set permissions
    UnableToSetPermissions(io::Error),
    /// Failed to retrieve file metadata
    UnableToRetrieveMetadata(io::Error),
    /// Failed to write bytes to file
    UnableToWriteFile(io::Error),
    /// Failed to obtain file path
    UnableToObtainFilePath,
    /// Failed to retrieve ACL for file
    UnableToRetrieveACL(u32),
    /// Failed to enumerate ACL entries
    UnableToEnumerateACLEntries(u32),
    /// Failed to add new ACL entry
    UnableToAddACLEntry(String),
    /// Failed to remove ACL entry
    UnableToRemoveACLEntry(String),
}

/// Creates a file with `600 (-rw-------)` permissions.
pub fn create_with_600_perms<P: AsRef<Path>>(path: P, bytes: &[u8]) -> Result<(), Error> {
    let path = path.as_ref();
    let mut file = File::create(&path).map_err(Error::UnableToCreateFile)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = file
            .metadata()
            .map_err(Error::UnableToRetrieveMetadata)?
            .permissions();
        perm.set_mode(0o600);
        file.set_permissions(perm)
            .map_err(Error::UnableToSetPermissions)?;
    }

    file.write_all(bytes)
        .map_err(Error::UnableToWriteFile)?;
    #[cfg(windows)]
    {
        restrict_file_permissions(path)?;
    }

    Ok(())
}

pub fn restrict_file_permissions<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = File::open(path.as_ref()).map_err(Error::UnableToOpenFile)?;
        let mut perm = file
            .metadata()
            .map_err(Error::UnableToRetrieveMetadata)?
            .permissions();
        perm.set_mode(0o600);
        file.set_permissions(perm)
            .map_err(Error::UnableToSetPermissions)?;
    }

    #[cfg(windows)]
    {
        use winapi::um::winnt::PSID;
        use windows_acl::acl::{AceType, ACL};
        use windows_acl::helper::sid_to_string;

        let path_str = path
            .as_ref()
            .to_str()
            .ok_or(Error::UnableToObtainFilePath)?;
        let mut acl =
            ACL::from_file_path(&path_str, false).map_err(|e| Error::UnableToRetrieveACL(e))?;

        let owner_sid_str = "S-1-3-4";
        let owner_sid = windows_acl::helper::string_to_sid(owner_sid_str).unwrap();

        let entries = acl
            .all()
            .map_err(|e| Error::UnableToEnumerateACLEntries(e))?;

        // add single entry for file owner
        acl.add_entry(
            owner_sid.as_ptr() as PSID,
            AceType::AccessAllow,
            0,
            0x1f01ff,
        )
        .map_err(|code| {
            Error::UnableToAddACLEntry(format!(
                "Failed to add ACL entry for SID {} error={}",
                owner_sid_str, code
            ))
        })?;
        // remove all AccessAllow entries from the file that aren't the owner_sid
        for entry in &entries {
            if let Some(ref entry_sid) = entry.sid {
                let entry_sid_str = sid_to_string((*entry_sid).as_ptr() as PSID)
                    .unwrap_or_else(|_| "BadFormat".to_string());
                if entry_sid_str != owner_sid_str {
                    acl.remove(
                        (*entry_sid).as_ptr() as PSID,
                        Some(AceType::AccessAllow),
                        None,
                    )
                    .map_err(|_| {
                        Error::UnableToRemoveACLEntry(format!(
                            "Failed to remove ACL entry for SID {}",
                            entry_sid_str
                        ))
                    })?;
                }
            }
        }
    }

    Ok(())
}
