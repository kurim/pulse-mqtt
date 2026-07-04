using System;
using System.Threading;
using System.Threading.Tasks;
using MQTTnet;
using MQTTnet.Protocol;
using MQTTnet.Server;

namespace PulseMQTT;

/// <summary>
/// Betreibt einen vollständigen MQTT-Broker im eigenen Prozess (kein separater
/// Mosquitto-Server mehr nötig). Das CYD-Board (oder ein anderer MQTT-Client,
/// z. B. mit Pulse ESP32-Firmware) verbindet sich direkt mit dieser App.
/// </summary>
public sealed class MqttBrokerService
{
    private readonly MqttServerFactory _factory = new();
    private MqttServer? _server;
    private int _connectedClients;

    public int ConnectedClients => _connectedClients;
    public bool IsRunning => _server?.IsStarted ?? false;

    public async Task StartAsync(int port, string? username = null, string? password = null)
    {
        await StopAsync();

        var options = new MqttServerOptionsBuilder()
            .WithDefaultEndpoint()
            .WithDefaultEndpointPort(port)
            .Build();

        _server = _factory.CreateMqttServer(options);

        _server.ClientConnectedAsync += _ =>
        {
            Interlocked.Increment(ref _connectedClients);
            return Task.CompletedTask;
        };

        _server.ClientDisconnectedAsync += _ =>
        {
            Interlocked.Decrement(ref _connectedClients);
            return Task.CompletedTask;
        };

        if (!string.IsNullOrEmpty(username))
        {
            _server.ValidatingConnectionAsync += args =>
            {
                if (args.UserName != username || args.Password != password)
                {
                    args.ReasonCode = MqttConnectReasonCode.BadUserNameOrPassword;
                }
                return Task.CompletedTask;
            };
        }

        await _server.StartAsync();
    }

    public async Task StopAsync()
    {
        if (_server is null)
        {
            return;
        }

        if (_server.IsStarted)
        {
            await _server.StopAsync();
        }

        _server = null;
        _connectedClients = 0;
    }

    /// <summary>
    /// Veröffentlicht eine Nachricht direkt im Broker (ohne über einen
    /// zusätzlichen internen MQTT-Client laufen zu müssen).
    /// </summary>
    public async Task PublishAsync(string topic, string jsonPayload)
    {
        if (_server is null || !_server.IsStarted)
        {
            return;
        }

        var message = new MqttApplicationMessageBuilder()
            .WithTopic(topic)
            .WithPayload(jsonPayload)
            .WithRetainFlag(false)
            .Build();

        await _server.InjectApplicationMessage(
            new InjectedMqttApplicationMessage(message) { SenderClientId = "PulseMQTT" });
    }
}
