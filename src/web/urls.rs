pub(crate) enum Urls {
    HealthCheck,
    Host,
    Hosts,
    HostGroup,
    HostGroups,
    Index,
    Login,
    Logout,
    Metrics,
    RpLogout,
    Profile,
    Service,
    Services,
    ServiceCheck,
    Static,
    Tools,
    ToolsExportDb,
}

impl AsRef<str> for Urls {
    fn as_ref(&self) -> &str {
        match self {
            Self::HealthCheck => "/healthcheck",
            Self::Host => "/host",
            Self::Hosts => "/hosts",
            Self::HostGroup => "/host_group",
            Self::HostGroups => "/host_groups",
            Self::Index => "/",
            Self::Login => "/auth/login",
            Self::Logout => "/auth/logout",
            Self::Metrics => "/metrics",
            Self::RpLogout => "/auth/rp-logout",
            Self::Profile => "/profile",
            Self::Service => "/service",
            Self::Services => "/services",
            Self::ServiceCheck => "/service_check",
            Self::Static => "/static",
            Self::Tools => "/tools",
            Self::ToolsExportDb => "/tools/db_export",
        }
    }
}

impl std::fmt::Display for Urls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_ref().fmt(f)
    }
}
