//! Picking an Android view (button, text, image...) on the game's screen:
//! Android's own view list (`uiautomator dump`), the smallest view under a
//! point, and the layout file that declares it. Games that draw everything
//! themselves show up as one drawing surface; their objects are found
//! through live positions instead.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiNode {
    /// `android.widget.Button`, `android.view.SurfaceView`...
    pub class: String,
    /// `com.game:id/play`, or empty.
    pub resource_id: String,
    pub text: String,
    pub description: String,
    pub package: String,
    /// left, top, right, bottom in device pixels.
    pub bounds: [i32; 4],
    /// The game draws here itself (a surface), so there's no view to edit.
    pub surface: bool,
    /// The layout file declaring this view, relative to the project's
    /// unpacked folder (`apktool/res/layout/main.xml`).
    pub layout: Option<String>,
}

fn attr(tag: &str, name: &str) -> String {
    let key = format!(" {name}=\"");
    let Some(start) = tag.find(&key).map(|i| i + key.len()) else { return String::new() };
    let value = &tag[start..];
    let value = &value[..value.find('"').unwrap_or(value.len())];
    value.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&apos;", "'").replace("&amp;", "&")
}

/// `[0,48][720,1280]`
fn parse_bounds(text: &str) -> Option<[i32; 4]> {
    let nums: Vec<i32> = text
        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    (nums.len() == 4).then(|| [nums[0], nums[1], nums[2], nums[3]])
}

/// Every view in a `uiautomator dump`.
pub fn parse_dump(xml: &str) -> Vec<UiNode> {
    let mut out = Vec::new();
    for part in xml.split("<node ").skip(1) {
        let tag = format!(" {}", &part[..part.find('>').unwrap_or(part.len())]);
        let Some(bounds) = parse_bounds(&attr(&tag, "bounds")) else { continue };
        let class = attr(&tag, "class");
        let (resource_id, text) = (attr(&tag, "resource-id"), attr(&tag, "text"));
        // A game's own view shows up as a bare `android.view.View`.
        let surface = ["SurfaceView", "GLSurfaceView", "TextureView"].iter().any(|s| class.ends_with(s))
            || (class == "android.view.View" && resource_id.is_empty() && text.is_empty());
        out.push(UiNode {
            surface,
            class,
            resource_id,
            text,
            description: attr(&tag, "content-desc"),
            package: attr(&tag, "package"),
            bounds,
            layout: None,
        });
    }
    out
}

/// The smallest view containing the point, preferring the game's own views
/// over system ones (status bar, keyboard).
pub fn view_at(nodes: &[UiNode], x: i32, y: i32, package: &str) -> Option<UiNode> {
    let inside = |n: &&UiNode| n.bounds[0] <= x && x < n.bounds[2] && n.bounds[1] <= y && y < n.bounds[3];
    let area = |n: &&UiNode| (n.bounds[2] - n.bounds[0]) as i64 * (n.bounds[3] - n.bounds[1]) as i64;
    let hits: Vec<&UiNode> = nodes.iter().filter(inside).collect();
    let own: Vec<&UiNode> = hits.iter().copied().filter(|n| n.package == package).collect();
    let pool = if own.is_empty() { hits } else { own };
    // Children come after their parents, so on a tie the deepest view wins.
    pool.into_iter().rev().min_by_key(|n| area(n)).cloned()
}

/// The layout file that declares `@+id/<name>` for a `pkg:id/name` resource ID.
pub fn find_layout(unpacked: &Path, resource_id: &str) -> Option<String> {
    let name = resource_id.rsplit('/').next().filter(|n| !n.is_empty())?;
    let needle = format!("\"@+id/{name}\"");
    let res = unpacked.join("apktool").join("res");
    let mut dirs: Vec<_> = std::fs::read_dir(&res)
        .ok()?
        .flatten()
        .filter(|d| d.file_name().to_string_lossy().starts_with("layout"))
        .map(|d| d.path())
        .collect();
    // Plain `layout` before `layout-land` and the like.
    dirs.sort();
    for dir in dirs {
        let mut files: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().map(|f| f.path()).collect();
        files.sort();
        for file in files {
            if std::fs::read_to_string(&file).is_ok_and(|t| t.contains(&needle)) {
                let rel = file.strip_prefix(unpacked).ok()?;
                return Some(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0">
<node index="0" text="" resource-id="" class="android.widget.FrameLayout" package="com.g" content-desc="" bounds="[0,0][720,1280]">
<node index="0" text="" resource-id="com.g:id/game" class="android.view.SurfaceView" package="com.g" content-desc="" bounds="[0,0][720,1280]" />
<node index="1" text="Play &amp; win" resource-id="com.g:id/play" class="android.widget.Button" package="com.g" content-desc="" bounds="[100,600][620,700]" />
</node>
<node index="2" text="" resource-id="" class="android.view.View" package="com.g" content-desc="" bounds="[0,1200][720,1280]" />
<node index="0" text="" resource-id="com.android.systemui:id/clock" class="android.widget.TextView" package="com.android.systemui" content-desc="12:00" bounds="[0,0][80,40]" />
</hierarchy>"#;

    #[test]
    fn picks_the_smallest_view_of_the_game() {
        let nodes = parse_dump(DUMP);
        assert_eq!(nodes.len(), 5);
        assert!(view_at(&nodes, 300, 1250, "com.g").unwrap().surface, "a bare View is drawn by the game");
        let button = view_at(&nodes, 300, 650, "com.g").unwrap();
        assert_eq!((button.class.as_str(), button.text.as_str()), ("android.widget.Button", "Play & win"));
        assert_eq!(button.bounds, [100, 600, 620, 700]);
        let surface = view_at(&nodes, 300, 900, "com.g").unwrap();
        assert!(surface.surface);
        // The game's view wins over the system clock drawn on top of it.
        assert_eq!(view_at(&nodes, 10, 10, "com.g").unwrap().resource_id, "com.g:id/game");
        assert_eq!(view_at(&nodes, 10, 10, "other").unwrap().package, "com.android.systemui");
        assert_eq!(view_at(&nodes, 900, 10, "com.g"), None);
    }

    #[test]
    fn finds_the_layout_declaring_a_view() {
        let dir = std::env::temp_dir().join(format!("gamex-ui-{}", std::process::id()));
        let layout = dir.join("apktool/res/layout");
        std::fs::create_dir_all(&layout).unwrap();
        std::fs::create_dir_all(dir.join("apktool/res/layout-land")).unwrap();
        std::fs::write(layout.join("menu.xml"), r#"<Button android:id="@+id/play" />"#).unwrap();
        std::fs::write(dir.join("apktool/res/layout-land/menu.xml"), r#"<Button android:id="@+id/play" />"#).unwrap();
        assert_eq!(find_layout(&dir, "com.g:id/play").as_deref(), Some("apktool/res/layout/menu.xml"));
        assert_eq!(find_layout(&dir, "com.g:id/pla"), None);
        assert_eq!(find_layout(&dir, ""), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
