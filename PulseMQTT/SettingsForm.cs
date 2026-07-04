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
    private readonly TextBox       _usernameInput;
    private readonly TextBox       _passwordInput;
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
        ClientSize      = new Size(380, 255);

        var layout = new TableLayoutPanel
        {
            Left = 12, Top = 12, Width = 356,
            ColumnCount = 2, RowCount = 7, AutoSize = true
        };
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 165));
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 191));
        for (int i = 0; i < 7; i++)
            layout.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));

        layout.Controls.Add(MkLabel(Localization.T("Settings.Port")), 0, 0);
        _portInput = new NumericUpDown
            { Minimum = 1, Maximum = 65535, Value = current.MqttPort, Width = 100 };
        layout.Controls.Add(_portInput, 1, 0);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Topic")), 0, 1);
        _topicInput = new TextBox { Text = current.MqttTopic, Width = 185 };
        layout.Controls.Add(_topicInput, 1, 1);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Username")), 0, 2);
        _usernameInput = new TextBox { Text = current.MqttUsername, Width = 185 };
        layout.Controls.Add(_usernameInput, 1, 2);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Password")), 0, 3);
        _passwordInput = new TextBox
            { Text = current.MqttPassword, Width = 185, UseSystemPasswordChar = true };
        layout.Controls.Add(_passwordInput, 1, 3);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Interval")), 0, 4);
        _intervalInput = new NumericUpDown
        {
            Minimum = 0.5m, Maximum = 60m, DecimalPlaces = 1, Increment = 0.5m,
            Value = (decimal)current.UpdateIntervalSeconds, Width = 100
        };
        layout.Controls.Add(_intervalInput, 1, 4);

        layout.Controls.Add(MkLabel(Localization.T("Settings.AutoStart")), 0, 5);
        _autoStartInput = new CheckBox { Checked = current.StartWithWindows };
        layout.Controls.Add(_autoStartInput, 1, 5);

        layout.Controls.Add(MkLabel(Localization.T("Settings.Language")), 0, 6);
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
        layout.Controls.Add(_languageInput, 1, 6);

        Controls.Add(layout);

        var hint = new Label
        {
            Text      = Localization.T("Settings.Hint"),
            AutoSize  = false,
            Left = 12, Width = 356, Height = 28, Top = layout.Bottom + 6,
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
                MqttPort              = (int)_portInput.Value,
                MqttTopic             = string.IsNullOrWhiteSpace(_topicInput.Text) ? "pulsemqtt/hwinfo" : _topicInput.Text.Trim(),
                MqttUsername          = _usernameInput.Text.Trim(),
                MqttPassword          = _passwordInput.Text,
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

    private static Label MkLabel(string t) => new()
        { Text = t, AutoSize = true, Anchor = AnchorStyles.Left | AnchorStyles.Top, Margin = new Padding(0, 8, 8, 0) };

    private sealed record LanguageItem(string Code, string DisplayName)
    {
        public override string ToString() => DisplayName;
    }
}
