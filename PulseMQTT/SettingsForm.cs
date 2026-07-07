using System;
using System.Drawing;
using System.IO.Ports;
using System.Windows.Forms;

namespace PulseMQTT;

public sealed class SettingsForm : Form
{
    private readonly CheckBox      _useMqttInput;
    private readonly NumericUpDown _portInput;
    private readonly TextBox       _topicInput;
    private readonly TextBox       _usernameInput;
    private readonly TextBox       _passwordInput;
    private readonly CheckBox      _useSerialInput;
    private readonly ComboBox      _serialPortInput;
    private readonly Button        _serialRefreshButton;
    private readonly ComboBox      _serialModeInput;
    private readonly NumericUpDown _intervalInput;
    private readonly CheckBox      _autoStartInput;
    private readonly ComboBox      _languageInput;

    public AppSettings Result { get; private set; }

    public SettingsForm(AppSettings current)
    {
        Result = current.Clone();

        Text            = Localization.T("Settings.Title");
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox     = false;
        MinimizeBox     = false;
        ShowInTaskbar   = false;
        StartPosition   = FormStartPosition.CenterScreen;
        AutoScaleMode   = AutoScaleMode.Dpi;
        ClientSize      = new Size(460, 255);

        const int rowCount = 11;
        var layout = new TableLayoutPanel
        {
            Left = 12, Top = 12, Width = 436,
            ColumnCount = 2, RowCount = rowCount, AutoSize = true
        };
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 165));
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 271));
        for (int i = 0; i < rowCount; i++)
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));

        layout.Controls.Add(MkLabel(Localization.T("Settings.UseMqtt")), 0, 0);
        _useMqttInput = new CheckBox { Checked = current.UseMqtt };
        layout.Controls.Add(_useMqttInput, 1, 0);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Port")), 0, 1);
        _portInput = new NumericUpDown
            { Minimum = 1, Maximum = 65535, Value = current.MqttPort, Width = 100 };
        layout.Controls.Add(_portInput, 1, 1);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Topic")), 0, 2);
        _topicInput = new TextBox { Text = current.MqttTopic, Width = 185 };
        layout.Controls.Add(_topicInput, 1, 2);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Username")), 0, 3);
        _usernameInput = new TextBox { Text = current.MqttUsername, Width = 185 };
        layout.Controls.Add(_usernameInput, 1, 3);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Password")), 0, 4);
        _passwordInput = new TextBox
            { Text = current.MqttPassword, Width = 185, UseSystemPasswordChar = true };
        layout.Controls.Add(_passwordInput, 1, 4);

        layout.Controls.Add(MkLabel(Localization.T("Settings.UseSerial")), 0, 5);
        _useSerialInput = new CheckBox { Checked = current.UseSerial };
        layout.Controls.Add(_useSerialInput, 1, 5);

        layout.Controls.Add(MkLabel(Localization.T("Settings.SerialPort")), 0, 6);
        var serialPanel = new FlowLayoutPanel
            { FlowDirection = FlowDirection.LeftToRight, AutoSize = true, WrapContents = false };
        _serialPortInput = new ComboBox { Width = 160, DropDownStyle = ComboBoxStyle.DropDownList };
        _serialRefreshButton = new Button
        {
            Text = "↻", // ⟳ Refresh-Symbol statt Text, damit der Button nicht abgeschnitten wird
            Width = 32, Margin = new Padding(6, 0, 0, 0),
            Font = new Font(Font!.FontFamily, 11f)
        };
        var refreshTip = new ToolTip();
        refreshTip.SetToolTip(_serialRefreshButton, Localization.T("Settings.SerialPort.Refresh"));
        _serialRefreshButton.Click += (_, _) => RefreshSerialPorts(current.SerialPortName);
        serialPanel.Controls.Add(_serialPortInput);
        serialPanel.Controls.Add(_serialRefreshButton);
        layout.Controls.Add(serialPanel, 1, 6);

        layout.Controls.Add(MkLabel(Localization.T("Settings.SerialMode")), 0, 7);
        _serialModeInput = new ComboBox { Width = 265, DropDownStyle = ComboBoxStyle.DropDownList };
        _serialModeInput.Items.AddRange(
        [
            new SerialModeItem(SerialConnectionMode.UsbSerialJtag, Localization.T("Settings.SerialMode.UsbSerialJtag")),
            new SerialModeItem(SerialConnectionMode.Uart0,          Localization.T("Settings.SerialMode.Uart0")),
        ]);
        _serialModeInput.SelectedIndex = current.SerialMode == SerialConnectionMode.Uart0 ? 1 : 0;
        layout.Controls.Add(_serialModeInput, 1, 7);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Interval")), 0, 8);
        _intervalInput = new NumericUpDown
        {
            Minimum = 0.5m, Maximum = 60m, DecimalPlaces = 1, Increment = 0.5m,
            Value = (decimal)current.UpdateIntervalSeconds, Width = 100
        };
        layout.Controls.Add(_intervalInput, 1, 8);

        layout.Controls.Add(MkLabel(Localization.T("Settings.AutoStart")), 0, 9);
        _autoStartInput = new CheckBox { Checked = current.StartWithWindows };
        layout.Controls.Add(_autoStartInput, 1, 9);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Language")), 0, 10);
        _languageInput = new ComboBox { Width = 185, DropDownStyle = ComboBoxStyle.DropDownList };
        _languageInput.Items.AddRange(
        [
            new LanguageItem("auto", Localization.T("Settings.Language.Auto")),
            new LanguageItem("de",   Localization.T("Settings.Language.German")),
            new LanguageItem("en",   Localization.T("Settings.Language.English")),
        ]);
        _languageInput.SelectedIndex = current.Language switch
        {
            "de" => 1,
            "en" => 2,
            _ => 0
        };
        layout.Controls.Add(_languageInput, 1, 10);

        Controls.Add(layout);

        RefreshSerialPorts(current.SerialPortName);
        UpdateFieldAvailability();
        _useMqttInput.CheckedChanged += (_, _) => UpdateFieldAvailability();
        _useSerialInput.CheckedChanged += (_, _) => UpdateFieldAvailability();

        var hint = new Label
        {
            Text      = Localization.T("Settings.Hint") + "\n" + Localization.T("Settings.SerialHint"),
            AutoSize  = false,
            Left = 12, Width = 436, Height = 56, Top = layout.Bottom + 6,
            ForeColor = SystemColors.GrayText
        };
        Controls.Add(hint);

        var cancelButton = new Button
            { Text = Localization.T("Button.Cancel"), DialogResult = DialogResult.Cancel, Width = 100 };
        cancelButton.Left = ClientSize.Width - cancelButton.Width - 12;
        cancelButton.Top  = hint.Bottom + 8;

        var okButton = new Button
            { Text = Localization.T("Button.Save"), DialogResult = DialogResult.OK, Width = 100 };
        okButton.Left = cancelButton.Left - okButton.Width - 8;
        okButton.Top  = cancelButton.Top;

        okButton.Click += (_, _) =>
        {
            Result = new AppSettings
            {
                UseMqtt               = _useMqttInput.Checked,
                MqttPort              = (int)_portInput.Value,
                MqttTopic             = string.IsNullOrWhiteSpace(_topicInput.Text) ? "pulsemqtt/hwinfo" : _topicInput.Text.Trim(),
                MqttUsername          = _usernameInput.Text.Trim(),
                MqttPassword          = _passwordInput.Text,
                UseSerial             = _useSerialInput.Checked,
                SerialPortName        = _serialPortInput.SelectedItem as string ?? "",
                SerialMode            = ((SerialModeItem)_serialModeInput.SelectedItem!).Mode,
                UpdateIntervalSeconds = (double)_intervalInput.Value,
                StartWithWindows      = _autoStartInput.Checked,
                Language              = ((LanguageItem)_languageInput.SelectedItem!).Code,
                EnabledSensors        = current.EnabledSensors  // Sensor-Wahl bleibt erhalten
            };
        };

        Controls.Add(okButton);
        Controls.Add(cancelButton);
        ClientSize   = new Size(ClientSize.Width, okButton.Bottom + 12);
        AcceptButton = okButton;
        CancelButton = cancelButton;
    }

    private void RefreshSerialPorts(string? selected)
    {
        _serialPortInput.Items.Clear();
        _serialPortInput.Items.AddRange(SerialPort.GetPortNames());
        if (!string.IsNullOrEmpty(selected) && !_serialPortInput.Items.Contains(selected))
            _serialPortInput.Items.Add(selected);

        _serialPortInput.SelectedItem = selected;
        if (_serialPortInput.SelectedIndex < 0 && _serialPortInput.Items.Count > 0)
            _serialPortInput.SelectedIndex = 0;
    }

    private void UpdateFieldAvailability()
    {
        var mqtt = _useMqttInput.Checked;
        _portInput.Enabled     = mqtt;
        _topicInput.Enabled    = mqtt;
        _usernameInput.Enabled = mqtt;
        _passwordInput.Enabled = mqtt;

        var serial = _useSerialInput.Checked;
        _serialPortInput.Enabled     = serial;
        _serialRefreshButton.Enabled = serial;
        _serialModeInput.Enabled     = serial;
    }

    private static Label MkLabel(string t) => new()
        { Text = t, AutoSize = true, Anchor = AnchorStyles.Left | AnchorStyles.Top, Margin = new Padding(0, 8, 8, 0) };

    private sealed record LanguageItem(string Code, string DisplayName)
    {
        public override string ToString() => DisplayName;
    }

    private sealed record SerialModeItem(SerialConnectionMode Mode, string DisplayName)
    {
        public override string ToString() => DisplayName;
    }
}
