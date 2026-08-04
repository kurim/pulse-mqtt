using System.Drawing;

namespace PulseMQTT;

/// <summary>
/// Lädt das App-Icon aus der eingebetteten Ressource.
/// Wird für Tray-Icon und alle Fenster (Titelleiste/Taskbar) verwendet.
/// </summary>
internal static class AppIcon
{
    private static Icon? _cached;

    /// <summary>
    /// Gibt eine app-weit geteilte Icon-Instanz zurück. Aufrufer dürfen sie
    /// NICHT disposen – vorher erzeugte jeder Aufruf ein eigenes GDI-Handle,
    /// das nie freigegeben wurde.
    /// </summary>
    public static Icon Load()
    {
        if (_cached is not null) return _cached;

        using var stream = typeof(AppIcon).Assembly
            .GetManifestResourceStream("PulseMQTT.app.ico");
        _cached = stream is not null ? new Icon(stream) : SystemIcons.Application;
        return _cached;
    }
}
