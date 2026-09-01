use shared::{
    State,
    models::{
        UpdatableModel,
        nest_egg::NestEgg,
        server::{Server, UpdateServerOptions},
    },
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppliedRuntime {
    pub loader: String,
    pub minecraft: Option<String>,
    pub loader_version: Option<String>,
    pub java: u8,
    pub egg_name: Option<String>,
    pub egg_uuid: Option<uuid::Uuid>,
    pub startup: String,
    pub image: Option<String>,
}

pub fn normalize_loader(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.contains("neoforge") || normalized == "neo forge" {
        Some("neoforge")
    } else if normalized.contains("forge") {
        Some("forge")
    } else if normalized.contains("fabric") {
        Some("fabric")
    } else if normalized.contains("quilt") {
        Some("quilt")
    } else if normalized.contains("paper") {
        Some("paper")
    } else if normalized.contains("purpur") {
        Some("purpur")
    } else if normalized.contains("spigot") {
        Some("spigot")
    } else if normalized.contains("vanilla") {
        Some("vanilla")
    } else {
        None
    }
}

fn version_parts(mc: &str) -> (u32, u32, u32) {
    let mut parts = mc
        .split('.')
        .take(3)
        .map(|part| part.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

pub fn recommended_java(mc: Option<&str>, explicit: Option<u8>) -> u8 {
    if let Some(explicit) = explicit.filter(|value| *value >= 8) {
        return explicit;
    }

    let Some(mc) = mc else { return 21 };
    let (major, minor, patch) = version_parts(mc);
    if major != 1 {
        return 21;
    }

    if minor > 20 || (minor == 20 && patch >= 5) {
        21
    } else if minor >= 17 {
        17
    } else {
        8
    }
}

fn egg_candidates(loader: &str) -> &'static [&'static str] {
    match loader {
        "neoforge" => &["NeoForge", "Minecraft NeoForge", "Neo Forge"],
        "forge" => &["Forge", "Minecraft Forge"],
        "fabric" => &["Fabric", "Minecraft Fabric"],
        "quilt" => &["Quilt", "Minecraft Quilt"],
        "paper" => &["Paper", "PaperMC", "Minecraft Paper"],
        "purpur" => &["Purpur", "Minecraft Purpur"],
        "spigot" => &["Spigot", "Minecraft Spigot"],
        "vanilla" => &["Vanilla Minecraft", "Minecraft Java", "Vanilla"],
        _ => &[],
    }
}

async fn find_egg(state: &State, server: &Server, loader: &str) -> Result<Option<NestEgg>, anyhow::Error> {
    for candidate in egg_candidates(loader) {
        if let Some(egg) = NestEgg::by_nest_uuid_name(&state.database, server.nest.uuid, candidate).await? {
            return Ok(Some(egg));
        }
    }
    Ok(None)
}

fn select_image(egg: &NestEgg, java: u8) -> Option<String> {
    let needle_a = format!("java {java}");
    let needle_b = format!("java_{java}");
    let needle_c = format!("java-{java}");
    let needle_d = format!("java{java}");

    for (label, image) in &egg.docker_images {
        let haystack = format!("{} {}", label, image).to_ascii_lowercase();
        if haystack.contains(&needle_a)
            || haystack.contains(&needle_b)
            || haystack.contains(&needle_c)
            || haystack.contains(&needle_d)
        {
            return Some(image.to_string());
        }
    }

    egg.docker_images.values().next().map(ToString::to_string)
}

fn concrete_startup(loader: &str, mc: Option<&str>) -> String {
    match loader {
        "forge" => {
            let old = mc.is_some_and(|mc| {
                let (_, minor, _) = version_parts(mc);
                minor > 0 && minor < 17
            });
            if old {
                "java -Xms128M -XX:MaxRAMPercentage=92.5 -jar server.jar nogui".into()
            } else {
                "java -Xms128M -XX:MaxRAMPercentage=92.5 @user_jvm_args.txt @unix_args.txt nogui".into()
            }
        }
        "neoforge" => "java -Xms128M -XX:MaxRAMPercentage=92.5 @user_jvm_args.txt @unix_args.txt nogui".into(),
        "fabric" | "quilt" | "paper" | "purpur" | "spigot" | "vanilla" => {
            "java -Xms128M -XX:MaxRAMPercentage=92.5 -jar server.jar nogui".into()
        }
        _ => "java -Xms128M -XX:MaxRAMPercentage=92.5 -jar server.jar nogui".into(),
    }
}

pub async fn apply(
    state: &State,
    server: &mut Server,
    loader: &str,
    minecraft: Option<&str>,
    loader_version: Option<&str>,
    explicit_java: Option<u8>,
) -> Result<AppliedRuntime, anyhow::Error> {
    let loader = normalize_loader(loader).unwrap_or("vanilla");
    let java = recommended_java(minecraft, explicit_java);
    let egg = find_egg(state, server, loader).await?;
    let startup = concrete_startup(loader, minecraft);

    let (egg_name, egg_uuid, image) = if let Some(egg) = egg {
        let image = select_image(&egg, java);
        (Some(egg.name.to_string()), Some(egg.uuid), image)
    } else {
        (None, None, Some(server.image.to_string()))
    };

    let options = UpdateServerOptions {
        egg_uuid,
        startup: Some(startup.clone().into()),
        image: image.clone().map(Into::into),
        ..Default::default()
    };

    server.update(state, options).await?;
    // Server::sync consumes self, so sync a clone while keeping the caller's
    // in-memory server available for the following native install call.
    server.clone().sync(&state.database).await?;

    Ok(AppliedRuntime {
        loader: loader.to_string(),
        minecraft: minecraft.map(ToString::to_string),
        loader_version: loader_version.map(ToString::to_string),
        java,
        egg_name,
        egg_uuid,
        startup,
        image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_loaders() {
        assert_eq!(normalize_loader("NeoForge"), Some("neoforge"));
        assert_eq!(normalize_loader("Minecraft Forge"), Some("forge"));
        assert_eq!(normalize_loader("fabric"), Some("fabric"));
    }

    #[test]
    fn chooses_java_from_minecraft_version() {
        assert_eq!(recommended_java(Some("1.16.5"), None), 8);
        assert_eq!(recommended_java(Some("1.20.1"), None), 17);
        assert_eq!(recommended_java(Some("1.20.5"), None), 21);
        assert_eq!(recommended_java(Some("1.21.1"), None), 21);
        assert_eq!(recommended_java(Some("1.20.1"), Some(21)), 21);
    }
}
