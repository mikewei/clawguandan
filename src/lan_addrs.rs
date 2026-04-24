//! Resolve Web UI `http://` base URLs from bind/listen info.
//! Includes both LAN and WAN addresses, with LAN sorted first.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[inline]
fn is_lan_ipv4(ip: &Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && (ip.is_private() || ip.is_link_local())
}

#[inline]
fn is_lan_ipv6(ip: &Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && ip.is_unique_local()
}

#[inline]
fn is_wan_ipv4(ip: &Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && !ip.is_private() && !ip.is_link_local()
}

#[inline]
fn is_wan_ipv6(ip: &Ipv6Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_unique_local()
        && !ip.is_unicast_link_local()
}

#[inline]
fn is_usable_ipv6(ip: &Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && !ip.is_unicast_link_local()
}

#[inline]
fn classify_url_tier(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) if is_lan_ipv4(&v4) => 0,
        IpAddr::V6(v6) if is_lan_ipv6(&v6) => 0,
        IpAddr::V4(v4) if is_wan_ipv4(&v4) => 1,
        IpAddr::V6(v6) if is_wan_ipv6(&v6) => 1,
        _ => 2,
    }
}

fn format_http_base(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(v4) => format!("http://{v4}:{port}"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
    }
}

#[cfg(unix)]
fn collect_ipv4_from_interfaces() -> Vec<Ipv4Addr> {
    use std::collections::HashSet;
    use std::ptr;

    let mut out: HashSet<Ipv4Addr> = HashSet::new();
    unsafe {
        let mut ifap: *mut libc::ifaddrs = ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return vec![];
        }
        let mut p = ifap;
        while !p.is_null() {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null() {
                let sa = ifa.ifa_addr.cast::<libc::sockaddr>();
                let family = (*sa).sa_family as libc::c_int;
                if family == libc::AF_INET {
                    let sin = ifa.ifa_addr.cast::<libc::sockaddr_in>();
                    let raw = (*sin).sin_addr.s_addr;
                    let ip = Ipv4Addr::from(u32::from_be(raw as u32));
                    if !ip.is_loopback() && !ip.is_unspecified() {
                        out.insert(ip);
                    }
                }
            }
            p = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    let mut v: Vec<_> = out.into_iter().collect();
    v.sort();
    v
}

#[cfg(unix)]
fn collect_ipv6_from_interfaces() -> Vec<Ipv6Addr> {
    use std::collections::HashSet;
    use std::ptr;

    let mut out: HashSet<Ipv6Addr> = HashSet::new();
    unsafe {
        let mut ifap: *mut libc::ifaddrs = ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return vec![];
        }
        let mut p = ifap;
        while !p.is_null() {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null() {
                let sa = ifa.ifa_addr.cast::<libc::sockaddr>();
                let family = (*sa).sa_family as libc::c_int;
                if family == libc::AF_INET6 {
                    let sin6 = ifa.ifa_addr.cast::<libc::sockaddr_in6>();
                    let ip6 = Ipv6Addr::from((*sin6).sin6_addr.s6_addr);
                    if is_usable_ipv6(&ip6) {
                        out.insert(ip6);
                    }
                }
            }
            p = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    let mut v: Vec<_> = out.into_iter().collect();
    v.sort();
    v
}

#[cfg(not(unix))]
fn collect_ipv4_from_interfaces() -> Vec<Ipv4Addr> {
    vec![]
}

#[cfg(not(unix))]
fn collect_ipv6_from_interfaces() -> Vec<Ipv6Addr> {
    vec![]
}

/// HTTP base URLs for the Web UI.
/// Returns both LAN and WAN addresses, ordered LAN first, then WAN.
pub fn lan_http_base_urls(listen_port: u16, bind_ip: IpAddr) -> Vec<String> {
    if listen_port == 0 {
        return vec![];
    }
    let mut ips: BTreeSet<IpAddr> = BTreeSet::new();
    match bind_ip {
        IpAddr::V4(v4) if v4.is_unspecified() => {
            for a in collect_ipv4_from_interfaces() {
                ips.insert(IpAddr::V4(a));
            }
        }
        IpAddr::V4(v4) => {
            if !v4.is_loopback() && !v4.is_unspecified() {
                ips.insert(IpAddr::V4(v4));
            }
        }
        IpAddr::V6(v6) if v6.is_unspecified() => {
            for a in collect_ipv4_from_interfaces() {
                ips.insert(IpAddr::V4(a));
            }
            for a in collect_ipv6_from_interfaces() {
                ips.insert(IpAddr::V6(a));
            }
        }
        IpAddr::V6(v6) => {
            if is_usable_ipv6(&v6) {
                ips.insert(IpAddr::V6(v6));
            }
        }
    }
    let mut out: Vec<_> = ips.into_iter().collect();
    out.sort_by(|a, b| {
        classify_url_tier(*a)
            .cmp(&classify_url_tier(*b))
            .then_with(|| a.cmp(b))
    });
    out.into_iter()
        .map(|ip| format_http_base(ip, listen_port))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_urls_skip_loopback_bind() {
        let u = lan_http_base_urls(22222, Ipv4Addr::LOCALHOST.into());
        assert!(u.is_empty());
    }

    #[test]
    fn lan_urls_private_bind() {
        let u = lan_http_base_urls(8080, Ipv4Addr::new(192, 168, 3, 10).into());
        assert_eq!(u, vec!["http://192.168.3.10:8080".to_string()]);
    }

    #[test]
    fn urls_include_public_bind_as_wan() {
        let u = lan_http_base_urls(80, Ipv4Addr::new(8, 8, 8, 8).into());
        assert_eq!(u, vec!["http://8.8.8.8:80".to_string()]);
    }

    #[test]
    fn classify_lan_and_wan_tiers() {
        assert_eq!(
            classify_url_tier(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2))),
            0
        );
        assert_eq!(classify_url_tier(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))), 1);
    }
}
