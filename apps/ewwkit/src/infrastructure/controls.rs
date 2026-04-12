use tokio::process::Command;

pub async fn volume_up(step: u8) -> anyhow::Result<()> {
    Command::new("pactl")
        .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("+{step}%")])
        .status()
        .await?;
    Ok(())
}

pub async fn volume_down(step: u8) -> anyhow::Result<()> {
    Command::new("pactl")
        .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("-{step}%")])
        .status()
        .await?;
    Ok(())
}

pub async fn volume_mute_toggle() -> anyhow::Result<()> {
    Command::new("pactl")
        .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
        .status()
        .await?;
    Ok(())
}

pub async fn brightness_up(step: u8) -> anyhow::Result<()> {
    Command::new("brightnessctl")
        .args(["set", &format!("+{step}%")])
        .status()
        .await?;
    Ok(())
}

pub async fn brightness_down(step: u8) -> anyhow::Result<()> {
    Command::new("brightnessctl")
        .args(["set", &format!("{step}%-")])
        .status()
        .await?;
    Ok(())
}

pub async fn volume_set(percent: u8) -> anyhow::Result<()> {
    Command::new("pactl")
        .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{percent}%")])
        .status()
        .await?;
    Ok(())
}

pub async fn brightness_set(percent: u8) -> anyhow::Result<()> {
    Command::new("brightnessctl")
        .args(["set", &format!("{percent}%")])
        .status()
        .await?;
    Ok(())
}
