/// Environment variables that move a client's credential store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigVariable {
    XdgConfigHome,
    GhConfigDir,
    GlabConfigDir,
    CargoHome,
    DockerConfig,
    Gnupghome,
    PiCodingAgentDir,
    CodexHome,
    ClaudeConfigDir,
    SnowflakeHome,
    DbtEngineProfilesDir,
    DbtProfilesDir,
}

impl ConfigVariable {
    pub const ALL: [Self; 12] = [
        Self::XdgConfigHome,
        Self::GhConfigDir,
        Self::GlabConfigDir,
        Self::CargoHome,
        Self::DockerConfig,
        Self::Gnupghome,
        Self::PiCodingAgentDir,
        Self::CodexHome,
        Self::ClaudeConfigDir,
        Self::SnowflakeHome,
        Self::DbtEngineProfilesDir,
        Self::DbtProfilesDir,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::XdgConfigHome => "XDG_CONFIG_HOME",
            Self::GhConfigDir => "GH_CONFIG_DIR",
            Self::GlabConfigDir => "GLAB_CONFIG_DIR",
            Self::CargoHome => "CARGO_HOME",
            Self::DockerConfig => "DOCKER_CONFIG",
            Self::Gnupghome => "GNUPGHOME",
            Self::PiCodingAgentDir => "PI_CODING_AGENT_DIR",
            Self::CodexHome => "CODEX_HOME",
            Self::ClaudeConfigDir => "CLAUDE_CONFIG_DIR",
            Self::SnowflakeHome => "SNOWFLAKE_HOME",
            Self::DbtEngineProfilesDir => "DBT_ENGINE_PROFILES_DIR",
            Self::DbtProfilesDir => "DBT_PROFILES_DIR",
        }
    }
}

/// An absolute config-location value from the hook's own environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDir {
    pub variable: ConfigVariable,
    pub path: String,
}
