use super::AppResult;
use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_NAME_STR_REVERSE_FLAG, CERT_X500_NAME_STR,
    CertGetNameStringW, CertNameToStrW,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO, WTD_CHOICE_FILE,
    WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};

pub(crate) fn verify_update_candidate(installed: &Path, candidate: &Path) -> AppResult<()> {
    let installed_publisher = authenticode_publisher(installed).map_err(|error| {
        format!("Die installierte App besitzt keine vertrauenswürdige Windows-Signatur: {error}")
    })?;
    let candidate_publisher = authenticode_publisher(candidate).map_err(|error| {
        format!("Das Update besitzt keine vertrauenswürdige Windows-Signatur: {error}")
    })?;

    if !publishers_match(&installed_publisher, &candidate_publisher) {
        return Err(format!(
            "Das Update wurde von einem anderen Herausgeber signiert ({} statt {}).",
            candidate_publisher.display_name, installed_publisher.display_name
        )
        .into());
    }

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct AuthenticodePublisher {
    display_name: String,
    distinguished_name: String,
}

fn authenticode_publisher(path: &Path) -> AppResult<AuthenticodePublisher> {
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide_path.as_ptr(),
        hFile: null_mut(),
        pgKnownSubject: null_mut(),
    };
    let mut trust_data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
        ..WINTRUST_DATA::default()
    };
    trust_data.Anonymous.pFile = &raw mut file_info;

    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            null_mut(),
            &raw mut action,
            (&raw mut trust_data).cast::<c_void>(),
        )
    };

    let publisher = if status == 0 {
        publisher_from_state(trust_data.hWVTStateData)
    } else {
        Err(format!(
            "Authenticode-Prüfung fehlgeschlagen (0x{:08X}).",
            status as u32
        )
        .into())
    };

    if !trust_data.hWVTStateData.is_null() {
        trust_data.dwStateAction = WTD_STATEACTION_CLOSE;
        unsafe {
            let _ = WinVerifyTrust(
                null_mut(),
                &raw mut action,
                (&raw mut trust_data).cast::<c_void>(),
            );
        }
    }

    publisher
}

fn publisher_from_state(state: *mut c_void) -> AppResult<AuthenticodePublisher> {
    if state.is_null() {
        return Err("Windows hat keine Signaturdetails geliefert.".into());
    }

    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return Err("Windows konnte den Signaturanbieter nicht auslesen.".into());
    }

    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return Err("Windows konnte den Herausgeber nicht auslesen.".into());
    }

    let signer = unsafe { &*signer };
    if signer.csCertChain == 0 || signer.pasCertChain.is_null() {
        return Err("Die Signatur enthält kein Herausgeberzertifikat.".into());
    }
    let certificate = unsafe { (*signer.pasCertChain).pCert };
    if certificate.is_null() {
        return Err("Die Signatur enthält kein lesbares Herausgeberzertifikat.".into());
    }
    let certificate_info = unsafe { (*certificate).pCertInfo };
    if certificate_info.is_null() {
        return Err("Das Herausgeberzertifikat enthält keine Identität.".into());
    }

    let length = unsafe {
        CertGetNameStringW(
            certificate,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            null_mut(),
            0,
        )
    };
    if length <= 1 {
        return Err("Das Herausgeberzertifikat besitzt keinen Anzeigenamen.".into());
    }

    let mut buffer = vec![0u16; length as usize];
    let written = unsafe {
        CertGetNameStringW(
            certificate,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            buffer.as_mut_ptr(),
            length,
        )
    };
    if written <= 1 {
        return Err("Der Herausgebername konnte nicht gelesen werden.".into());
    }

    let display_name = String::from_utf16(&buffer[..written.saturating_sub(1) as usize])?;
    if display_name.trim().is_empty() {
        return Err("Der Herausgebername ist leer.".into());
    }

    let subject = unsafe { &(*certificate_info).Subject };
    let string_type = CERT_X500_NAME_STR | CERT_NAME_STR_REVERSE_FLAG;
    let subject_length = unsafe {
        CertNameToStrW(
            (*certificate).dwCertEncodingType,
            subject,
            string_type,
            null_mut(),
            0,
        )
    };
    if subject_length <= 1 {
        return Err("Die vollständige Herausgeberidentität konnte nicht gelesen werden.".into());
    }
    let mut subject_buffer = vec![0u16; subject_length as usize];
    let subject_written = unsafe {
        CertNameToStrW(
            (*certificate).dwCertEncodingType,
            subject,
            string_type,
            subject_buffer.as_mut_ptr(),
            subject_length,
        )
    };
    if subject_written <= 1 {
        return Err("Die vollständige Herausgeberidentität ist leer.".into());
    }
    let distinguished_name =
        String::from_utf16(&subject_buffer[..subject_written.saturating_sub(1) as usize])?;

    Ok(AuthenticodePublisher {
        display_name,
        distinguished_name,
    })
}

fn publishers_match(expected: &AuthenticodePublisher, candidate: &AuthenticodePublisher) -> bool {
    expected
        .distinguished_name
        .trim()
        .eq_ignore_ascii_case(candidate.distinguished_name.trim())
}

#[cfg(test)]
mod tests {
    use super::{AuthenticodePublisher, authenticode_publisher, publishers_match};
    use std::path::Path;

    #[test]
    fn publisher_comparison_uses_full_certificate_identity() {
        let expected = AuthenticodePublisher {
            display_name: "SignPath Foundation".to_owned(),
            distinguished_name: " CN=SignPath Foundation, O=SignPath Foundation ".to_owned(),
        };
        let same = AuthenticodePublisher {
            display_name: "SignPath Foundation".to_owned(),
            distinguished_name: "cn=signpath foundation, o=signpath foundation".to_owned(),
        };
        let foreign = AuthenticodePublisher {
            display_name: "SignPath Foundation".to_owned(),
            distinguished_name: "CN=SignPath Foundation, O=Other Organization".to_owned(),
        };
        assert!(publishers_match(&expected, &same));
        assert!(!publishers_match(&expected, &foreign));
    }

    #[test]
    fn reads_publisher_from_embedded_signature_when_fixture_is_available() {
        let Some(path) = std::env::var_os("GPHOTOS_SIGNED_TEST_EXE") else {
            return;
        };
        let publisher = authenticode_publisher(Path::new(&path))
            .expect("embedded Windows signature must be valid");
        assert!(
            !publisher.display_name.trim().is_empty()
                && !publisher.distinguished_name.trim().is_empty(),
            "signed fixture publisher must not be empty"
        );
    }
}
