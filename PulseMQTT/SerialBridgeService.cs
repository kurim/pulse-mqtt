using System;
using System.IO.Ports;
using System.Text;
using System.Text.Json.Serialization;
using System.Threading.Tasks;

namespace PulseMQTT;

/// <summary>
/// Wie das Gerät physisch an den USB-Port angebunden ist. Bestimmt, ob DTR
/// gesetzt und eine Reset-Settle-Zeit abgewartet werden muss.
/// </summary>
[JsonConverter(typeof(JsonStringEnumConverter))]
public enum SerialConnectionMode
{
    /// <summary>
    /// Klassischer USB-Serial-Adapter (CH340/CP210x/FTDI o.ä.), der auf den
    /// physischen UART0-Pins (GPIO1/3) lauscht. Reagiert unabhängig von DTR
    /// und braucht keine Settle-Zeit.
    /// </summary>
    Uart0,

    /// <summary>
    /// Natives USB-CDC/"USB-Serial/JTAG"-Peripheriegerät, wie es z.B. der
    /// ESP32-C3 eingebaut hat (keine physischen UART0-Pins am USB-Port).
    /// DTR signalisiert "Terminal verbunden" und löst bei manchen Boards
    /// zusätzlich einen kurzen Reset über die Auto-Reset-Beschaltung aus.
    /// </summary>
    UsbSerialJtag,
}

/// <summary>
/// Sendet Hardware-Daten als newline-getrennte JSON-Zeilen über einen
/// seriellen (USB-)Port – ein Peer zu <see cref="MqttBrokerService"/> für
/// Nutzer ohne WLAN/Broker. Baudrate/Framing sind fix auf die Pulse
/// ESP32-Firmware abgestimmt (115200 8N1, kein Handshake, ein JSON-Objekt
/// pro Zeile). UART0 (klassischer Adapter) und USB-Serial/JTAG (natives
/// USB-CDC, z.B. ESP32-C3) werden beide unterstützt, siehe
/// <see cref="SerialConnectionMode"/>.
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
    private const int OpenTimeoutMs  = 3000;
    private const int IoTimeoutMs    = 1000;
    private const int SettleDelayMs  = 1500;

    private SerialPort? _port;

    public bool IsOpen => _port?.IsOpen ?? false;
    public string? PortName => _port?.PortName;

    public async Task OpenAsync(string portName, SerialConnectionMode mode = SerialConnectionMode.UsbSerialJtag)
    {
        Close();

        if (string.IsNullOrWhiteSpace(portName))
            return;

        var isUsbSerialJtag = mode == SerialConnectionMode.UsbSerialJtag;

        var port = new SerialPort(portName, BaudRate, Parity.None, 8, StopBits.One)
        {
            Handshake   = Handshake.None,
            Encoding    = Encoding.UTF8,
            NewLine     = "\n",
            ReadTimeout  = IoTimeoutMs,
            WriteTimeout = IoTimeoutMs,
            // Natives USB-CDC ("USB-Serial/JTAG", z.B. ESP32-C3) liefert Daten
            // erst, sobald DTR gesetzt ist – das signalisiert "ein Terminal
            // ist verbunden" (Browser/Web Serial API und Terminalprogramme
            // setzen dies automatisch beim Öffnen, .NET SerialPort default
            // ist aus). Ein klassischer UART0-Adapter (CH340/CP210x) leitet
            // Daten unabhängig von DTR durch, daher hier aus lassen.
            // RTS bleibt in beiden Modi aus: auf vielen Boards ist RTS über
            // die Auto-Reset-Beschaltung mit GPIO0 verbunden – dauerhaft
            // gesetzt hält das Board im Bootloader-Modus fest, statt die
            // Firmware laufen zu lassen.
            DtrEnable = isUsbSerialJtag,
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

        if (isUsbSerialJtag)
        {
            // DTR löst auf vielen Boards beim Verbinden einen kurzen Reset
            // aus. Kurz warten, bis die Firmware wieder hochgefahren und die
            // USB-CDC-Pipe stabil ist, bevor der erste Write erfolgt – sonst
            // schlägt dieser mit einem low-level USB-Fehler fehl
            // ("Semaphore-Zeitlimit").
            await Task.Delay(SettleDelayMs);
        }

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
