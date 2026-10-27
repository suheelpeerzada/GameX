import AiSettings from "../components/AiSettings";
import ToolsPanel from "../components/ToolsPanel";

export default function SettingsScreen() {
  return (
    <div className="screen">
      <h1>Settings</h1>
      <ToolsPanel />
      <div className="settings-gap" />
      <AiSettings />
    </div>
  );
}
