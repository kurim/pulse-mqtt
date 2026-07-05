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

    private SerialPort? _port;

    public bool IsOpen => _port?.IsOpen ?? false;
    public string? PortName => _port?.PortName;

    public Task OpenAsync(string portName)
    {
        Close();

        if (string.IsNullOrWhiteSpace(portName))
            return Task.CompletedTask;

        _port = new SerialPort(portName, BaudRate, Parity.None, 8, StopBits.One)
        {
            Handshake = Handshake.None,
            Encoding  = Encoding.UTF8,
            NewLine   = "\n",
        };
        _port.Open();

        return Task.CompletedTask;
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
    public Task WriteLineAsync(string json)
    {
        if (_port is not { IsOpen: true })
            return Task.CompletedTask;

        _port.Write(json + "\n");
        return Task.CompletedTask;
    }
}
