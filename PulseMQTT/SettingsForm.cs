using System;
using System.Drawing;
using System.Windows.Forms;

namespace PulseMQTT;

public sealed class SettingsForm : Form
{
    private readonly NumericUpDown _portInput;
    private readonly TextBox       _topicInput;
    private readonly NumericUpDown _intervalInput;
    private readonly CheckBox      _autoStartInput;

    public AppSettings Result { get; private set; }

    public SettingsForm(AppSettings current)
    {
        Result = current.Clone();

        Text            = "PulseMQTT – Einstellungen";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox     = false;
        MinimizeBox     = false;
        ShowInTaskbar   = false;
        StartPosition   = FormStartPosition.CenterScreen;
        AutoScaleMode   = AutoScaleMode.Dpi;
        ClientSize      = new Size(380, 255);

        var layout = new TableLayoutPanel
        {
            Left = 12, Top = 12, Width = 356,
            ColumnCount = 2, RowCount = 4, AutoSize = true
        };
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 165));
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 191));
        for (int i = 0; i < 4; i++)
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));

        layout.Controls.Add(MkLabel("MQTT-Port:"), 0, 0);
        _portInput = new NumericUpDown
            { Minimum = 1, Maximum = 65535, Value = current.MqttPort, Width = 100 };
        layout.Controls.Add(_portInput, 1, 0);

        layout.Controls.Add(MkLabel("MQTT-Topic:"), 0, 1);
        _topicInput = new TextBox { Text = current.MqttTopic, Width = 185 };
        layout.Controls.Add(_topicInput, 1, 1);

        layout.Controls.Add(MkLabel("Update-Intervall (Sek.):"), 0, 2);
        _intervalInput = new NumericUpDown
        {
            Minimum = 0.5m, Maximum = 60m, DecimalPlaces = 1, Increment = 0.5m,
            Value = (decimal)current.UpdateIntervalSeconds, Width = 100
        };
        layout.Controls.Add(_intervalInput, 1, 2);

        layout.Controls.Add(MkLabel("Mit Windows starten:"), 0, 3);
        _autoStartInput = new CheckBox { Checked = current.StartWithWindows };
        layout.Controls.Add(_autoStartInput, 1, 3);

        Controls.Add(layout);

        var hint = new Label
        {
            Text      = "Topic muss mit der Firmware-Konfiguration übereinstimmen.",
            AutoSize  = false,
            Left = 12, Width = 356, Height = 28, Top = layout.Bottom + 6,
            ForeColor = SystemColors.GrayText
        };
        Controls.Add(hint);

        var cancelButton = new Button
            { Text = "Abbrechen", DialogResult = DialogResult.Cancel, Width = 100 };
        cancelButton.Left = ClientSize.Width - cancelButton.Width - 12;
        cancelButton.Top  = hint.Bottom + 8;

        var okButton = new Button
            { Text = "Speichern", DialogResult = DialogResult.OK, Width = 100 };
        okButton.Left = cancelButton.Left - okButton.Width - 8;
        okButton.Top  = cancelButton.Top;

        okButton.Click += (_, _) =>
        {
            Result = new AppSettings
            {
                MqttPort              = (int)_portInput.Value,
                MqttTopic             = string.IsNullOrWhiteSpace(_topicInput.Text) ? "pulsemqtt/hwinfo" : _topicInput.Text.Trim(),
                UpdateIntervalSeconds = (double)_intervalInput.Value,
                StartWithWindows      = _autoStartInput.Checked,
                EnabledSensors        = current.EnabledSensors  // Sensor-Wahl bleibt erhalten
            };
        };

        Controls.Add(okButton);
        Controls.Add(cancelButton);
        ClientSize   = new Size(ClientSize.Width, okButton.Bottom + 12);
        AcceptButton = okButton;
        CancelButton = cancelButton;
    }

    private static Label MkLabel(string t) => new()
        { Text = t, AutoSize = true, Anchor = AnchorStyles.Left | AnchorStyles.Top, Margin = new Padding(0, 8, 8, 0) };
}
