use crate::common::new_error;
use crate::config::DokodemoDoor;
use crate::proxy::Address;
use libc::c_int;
use std::mem;
use std::net::{SocketAddr, TcpListener};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

macro_rules! syscall {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        #[allow(unused_unsafe)]
        let res = unsafe { libc::$fn($($arg, )*) };
        if res == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}

#[cfg(unix)]
pub(crate) unsafe fn setsockopt<T>(
    fd: c_int,
    opt: c_int,
    val: c_int,
    payload: T,
) -> std::io::Result<()> {
    let payload = &payload as *const T as *const libc::c_void;
    syscall!(setsockopt(
        fd,
        opt,
        val,
        payload,
        mem::size_of::<T>() as libc::socklen_t,
    ))
    .map(|_| ())
}

pub(crate) fn build_dokodemo_door_listener(
    door: &mut DokodemoDoor,
    backlog: u32,
) -> std::io::Result<TcpListener> {
    let domain = match door.addr {
        Address::SocketAddress(SocketAddr::V4(_)) => socket2::Domain::IPV4,
        Address::SocketAddress(SocketAddr::V6(_)) => socket2::Domain::IPV6,
        Address::DomainNameAddress(_, _) => {
            return Err(new_error("unsupported dokodemo door listen addr type."));
        }
    };
    let socket = socket2::Socket::new(domain, socket2::Type::STREAM, Some(socket2::Protocol::TCP))?;
    socket.set_nonblocking(true)?;
    #[cfg(target_os = "linux")]
    {
        log::info!("set tproxy to {}", door.tproxy);
        socket.set_reuse_address(true)?;
        // For Go compatibility, also set reuse_port if available
        #[cfg(target_os = "linux")]
        {
            let _ = socket.set_reuse_port(true);
        }
        if domain == socket2::Domain::IPV6 {
            unsafe {
                setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_IPV6,
                    libc::IPV6_TRANSPARENT,
                    door.tproxy as c_int,
                )?;
            }
        } else {
            socket.set_ip_transparent(door.tproxy)?;
        }
        // For TPROXY, also need IP_RECVORIGDSTADDR for UDP, but for TCP we use SO_ORIGINAL_DST
    }
    let addr = door.addr.get_sock_addr().into();
    socket.bind(&addr)?;
    socket.listen(backlog as c_int)?;
    let std_listener = TcpListener::from(socket);
    Ok(std_listener)
}

/// For Go compatibility, retrieve original destination for TPROXY
/// This matches v2ray-core Go's behavior for dokodemo-door with tproxy
#[cfg(target_os = "linux")]
pub fn get_original_dst(stream: &tokio::net::TcpStream) -> std::io::Result<SocketAddr> {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    // Try IPv6 first, then IPv4
    // SO_ORIGINAL_DST = 80, same for IPv4 and IPv6 but different levels
    unsafe {
        // Try IPv6 original dst
        let mut dst: libc::sockaddr_storage = mem::zeroed();
        let mut len = mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let ret = libc::getsockopt(
            fd,
            libc::SOL_IPV6,
            80, // IP6T_SO_ORIGINAL_DST
            &mut dst as *mut _ as *mut libc::c_void,
            &mut len,
        );
        if ret == 0 {
            // Check family
            if dst.ss_family == libc::AF_INET6 as u16 {
                let sockaddr_in6: *const libc::sockaddr_in6 = &dst as *const _ as *const libc::sockaddr_in6;
                let ip = (*sockaddr_in6).sin6_addr;
                let port = u16::from_be((*sockaddr_in6).sin6_port);
                let ip_bytes = ip.s6_addr;
                let ipv6 = std::net::Ipv6Addr::from(ip_bytes);
                return Ok(SocketAddr::new(std::net::IpAddr::V6(ipv6), port));
            } else if dst.ss_family == libc::AF_INET as u16 {
                let sockaddr_in: *const libc::sockaddr_in = &dst as *const _ as *const libc::sockaddr_in;
                let ip = u32::from_be((*sockaddr_in).sin_addr.s_addr);
                let port = u16::from_be((*sockaddr_in).sin_port);
                let ipv4 = std::net::Ipv4Addr::from(ip);
                return Ok(SocketAddr::new(std::net::IpAddr::V4(ipv4), port));
            }
        }

        // Try IPv4 original dst
        let mut dst: libc::sockaddr_storage = mem::zeroed();
        let mut len = mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let ret = libc::getsockopt(
            fd,
            libc::SOL_IP,
            80, // SO_ORIGINAL_DST
            &mut dst as *mut _ as *mut libc::c_void,
            &mut len,
        );
        if ret == 0 {
            if dst.ss_family == libc::AF_INET as u16 {
                let sockaddr_in: *const libc::sockaddr_in = &dst as *const _ as *const libc::sockaddr_in;
                let ip = u32::from_be((*sockaddr_in).sin_addr.s_addr);
                let port = u16::from_be((*sockaddr_in).sin_port);
                let ipv4 = std::net::Ipv4Addr::from(ip);
                return Ok(SocketAddr::new(std::net::IpAddr::V4(ipv4), port));
            } else if dst.ss_family == libc::AF_INET6 as u16 {
                let sockaddr_in6: *const libc::sockaddr_in6 = &dst as *const _ as *const libc::sockaddr_in6;
                let ip = (*sockaddr_in6).sin6_addr;
                let port = u16::from_be((*sockaddr_in6).sin6_port);
                let ip_bytes = ip.s6_addr;
                let ipv6 = std::net::Ipv6Addr::from(ip_bytes);
                return Ok(SocketAddr::new(std::net::IpAddr::V6(ipv6), port));
            }
        }

        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
pub fn get_original_dst(_stream: &tokio::net::TcpStream) -> std::io::Result<SocketAddr> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "TPROXY not supported on this platform",
    ))
}
