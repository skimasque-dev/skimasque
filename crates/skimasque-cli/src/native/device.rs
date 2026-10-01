//! Attach an existing persistent user-owned Linux TUN, without configuring it.
use anyhow::{bail, Context};
pub struct Device {
    fd: tun_rs::AsyncDevice,
    pub name: String,
}
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.len() > 15
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        bail!("invalid TUN interface name");
    }
    Ok(())
}
impl Device {
    pub async fn attach(name: &str) -> anyhow::Result<Self> {
        validate_name(name)?;
        let c_name = std::ffi::CString::new(name)?;
        // SAFETY: validated NUL-terminated name; this is a read-only namespace-local query.
        let index = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if index == 0 {
            bail!("TUN interface must already exist");
        }
        tokio::fs::metadata("/dev/net/tun")
            .await
            .context("TUN kernel device missing")?;
        let fd = tun_rs::DeviceBuilder::new()
            .name(name)
            .inherit_enable_state()
            .offload(false)
            .multi_queue(false)
            .packet_information(false)
            .build_async()
            .context("attaching existing user-owned TUN with tun-rs")?;
        if unsafe { libc::if_nametoindex(c_name.as_ptr()) } != index {
            bail!("TUN changed during attachment");
        }
        if fd.name()? != name || fd.mtu()? != 1280 {
            bail!("native TUN requires exact interface and MTU 1280");
        }
        Ok(Self {
            fd,
            name: name.into(),
        })
    }
    pub async fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.fd.recv(buf).await
    }
    pub async fn write(&self, buf: &[u8]) -> std::io::Result<()> {
        let n = self.fd.send(buf).await?;
        if n != buf.len() {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        Ok(())
    }
}
