using System.Drawing;

namespace PulseMQTT;

/// <summary>
/// Lädt das App-Icon aus der eingebetteten Ressource.
/// Wird für Tray-Icon und alle Fenster (Titelleiste/Taskbar) verwendet.
/// </summary>
internal static class AppIcon
{
    public static Icon Load()
    {
        using var stream = typeof(AppIcon).Assembly
            .GetManifestResourceStream("PulseMQTT.app.ico");
        return stream is not null ? new Icon(stream) : SystemIcons.Application;
    }
}
