using System;
using System.IO.Ports;
using System.Text;
using System.Threading.Tasks;

namespace PulseMQTT;

/// <summary>
/// Sendet Hardware-Daten als newline-getrennte JSON-Zeilen über einen
/// seriellen (USB-)Port – ein Peer zu <see cref="MqttBrokerService"/> für
/// Nutzer ohne WLAN/Broker. Baudrate/Framing sind fix auf die Pulse
/// ESP32-Firmware abgestimmt (main/net/serial_handler.c: UART0, 115200 8N1,
/// kein Handshake, ein JSON-Objekt pro Zeile).
/// </summary>
public sealed class SerialBridgeService
{
    private const int BaudRate = 115200;

    /// <summary>
    /// Manche USB-Serial-Treiber (z.B. verwaiste/"Phantom"-COM-Ports oder
    /// Adapter mit hängendem CDC-Handshake) blockieren SerialPort.Open()
    /// bzw. .Write() unbegrenzt. Ohne diese Timeouts friert der aufrufende
    /// Thread (UI-Thread) komplett ein – bis hin zum nicht mehr reagierenden
    /// Tray-Kontextmenü, das sich nur per Taskmanager beenden lässt.
    /// </summary>
    private const int OpenTimeoutMs = 3000;
    private const int IoTimeoutMs   = 1000;

    private SerialPort? _port;

    public bool IsOpen => _port?.IsOpen ?? false;
    public string? PortName => _port?.PortName;

    public async Task OpenAsync(string portName)
    {
        Close();

        if (string.IsNullOrWhiteSpace(portName))
            return;

        var port = new SerialPort(portName, BaudRate, Parity.None, 8, StopBits.One)
        {
            Handshake   = Handshake.None,
            Encoding    = Encoding.UTF8,
            NewLine     = "\n",
            ReadTimeout  = IoTimeoutMs,
            WriteTimeout = IoTimeoutMs,
        };

        var openTask = Task.Run(port.Open);
        var finished = await Task.WhenAny(openTask, Task.Delay(OpenTimeoutMs));

        if (finished != openTask)
        {
            // Open() hängt (z.B. defekter/verwaister Treiber) – Port verwerfen
            // statt den Aufrufer (UI-Thread) auf unbestimmte Zeit zu blockieren.
            try { port.Dispose(); } catch { /* Port ggf. bereits entfernt */ }
            throw new TimeoutException($"Timeout beim Öffnen von {portName}.");
        }

        // Etwaige Exception aus Open() weiterreichen (z.B. "Zugriff verweigert").
        await openTask;

        _port = port;
    }

    public Task CloseAsync()
    {
        Close();
        return Task.CompletedTask;
    }

    private void Close()
    {
        if (_port is null) return;
        try
        {
            if (_port.IsOpen) _port.Close();
        }
        catch { /* Port ggf. bereits entfernt (Kabel gezogen) */ }
        finally
        {
            _port.Dispose();
            _port = null;
        }
    }

    /// <summary>Schreibt eine JSON-Zeile (mit abschließendem "\n") auf den Port.</summary>
    public async Task WriteLineAsync(string json)
    {
        var port = _port;
        if (port is not { IsOpen: true })
            return;

        try
        {
            await Task.Run(() => port.Write(json + "\n"));
        }
        catch
        {
            // Schreiben schlägt fehl (z.B. Gerät abgezogen, Timeout) –
            // Port schließen statt beim nächsten Tick erneut zu blockieren.
            Close();
            throw;
        }
    }
}
