using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;

namespace PulseMQTT;

/// <summary>
/// Dialog zur Auswahl und Konfiguration der zu publizierenden Sensoren.
/// Zeigt alle von LHM entdeckten Sensoren mit Live-Werten, aktuell aktiven MQTT-Keys
/// und Checkboxen für Aktivierung/Deaktivierung.
/// </summary>
public sealed class SensorPickerForm : Form
{
    private readonly List<AvailableSensor> _sensors;
    private readonly List<SensorEntry> _existingEntries;
    private readonly DataGridView _grid;
    private readonly System.Windows.Forms.Timer _refreshTimer;
    private readonly Func<Dictionary<string, float>> _getLiveValues;

    // Spalten-Indizes
    private const int ColEnabled  = 0;
    private const int ColHardware = 1;
    private const int ColType     = 2;
    private const int ColName     = 3;
    private const int ColMqttKey  = 4;
    private const int ColValue    = 5;

    public List<SensorEntry> Result { get; private set; } = [];

    public SensorPickerForm(
        List<AvailableSensor> sensors,
        List<SensorEntry> existingEntries,
        Func<Dictionary<string, float>> getLiveValues)
    {
        _sensors        = sensors;
        _existingEntries = existingEntries;
        _getLiveValues  = getLiveValues;

        Text            = Localization.T("Sensors.Title");
        FormBorderStyle = FormBorderStyle.Sizable;
        MinimumSize     = new Size(750, 450);
        Size            = new Size(860, 560);
        StartPosition   = FormStartPosition.CenterScreen;
        ShowInTaskbar   = false;

        // ── Layout ──────────────────────────────────────────────────────────
        var layout = new TableLayoutPanel
        {
            Dock       = DockStyle.Fill,
            RowCount   = 3,
            ColumnCount = 1
        };
        layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));      // Hinweiszeile
        layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));  // Grid
        layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));      // Buttons
        Controls.Add(layout);

        // ── Hinweistext ─────────────────────────────────────────────────────
        var hint = new Label
        {
            Text      = Localization.T("Sensors.Hint"),
            AutoSize  = false,
            Dock      = DockStyle.Fill,
            Height    = 36,
            Padding   = new Padding(6, 4, 6, 0),
            ForeColor = SystemColors.GrayText
        };
        layout.Controls.Add(hint, 0, 0);

        // ── DataGridView ────────────────────────────────────────────────────
        _grid = new DataGridView
        {
            Dock                  = DockStyle.Fill,
            AutoSizeColumnsMode   = DataGridViewAutoSizeColumnsMode.Fill,
            RowHeadersVisible     = false,
            AllowUserToAddRows    = false,
            AllowUserToDeleteRows = false,
            SelectionMode         = DataGridViewSelectionMode.FullRowSelect,
            BackgroundColor       = SystemColors.Window,
            BorderStyle           = BorderStyle.None,
            CellBorderStyle       = DataGridViewCellBorderStyle.SingleHorizontal,
            GridColor             = SystemColors.ControlLight
        };
        _grid.DefaultCellStyle.SelectionBackColor = Color.FromArgb(220, 235, 252);
        _grid.DefaultCellStyle.SelectionForeColor = SystemColors.ControlText;

        // Spalten definieren
        _grid.Columns.Add(new DataGridViewCheckBoxColumn
        {
            Name        = "Enabled",
            HeaderText  = "✓",
            Width       = 30,
            AutoSizeMode = DataGridViewAutoSizeColumnMode.None,
            Resizable   = DataGridViewTriState.False
        });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Hardware", HeaderText = Localization.T("Sensors.Col.Hardware"), FillWeight = 20, ReadOnly = true });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Type",     HeaderText = Localization.T("Sensors.Col.Type"),     FillWeight = 14, ReadOnly = true });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Sensor",   HeaderText = Localization.T("Sensors.Col.Sensor"),   FillWeight = 24, ReadOnly = true });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "MqttKey",  HeaderText = Localization.T("Sensors.Col.MqttKey"),  FillWeight = 20 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Value",    HeaderText = Localization.T("Sensors.Col.Value"),    FillWeight = 12, ReadOnly = true });

        _grid.Columns[ColMqttKey].DefaultCellStyle.BackColor = Color.FromArgb(255, 255, 230);

        layout.Controls.Add(_grid, 0, 1);
        PopulateGrid();

        // ── Button-Zeile ────────────────────────────────────────────────────
        var buttonPanel = new FlowLayoutPanel
        {
            Dock          = DockStyle.Fill,
            FlowDirection = FlowDirection.RightToLeft,
            Padding       = new Padding(6, 4, 6, 6),
            AutoSize      = true
        };

        var btnOk = new Button { Text = Localization.T("Button.Save"), Width = 100, DialogResult = DialogResult.OK };
        var btnCancel = new Button { Text = Localization.T("Button.Cancel"), Width = 100, DialogResult = DialogResult.Cancel };
        var btnDefault = new Button { Text = Localization.T("Button.Defaults"), Width = 120 };

        btnDefault.Click += (_, _) => ResetToDefaults();
        btnOk.Click      += (_, _) => CollectResult();

        buttonPanel.Controls.AddRange([btnCancel, btnOk, btnDefault]);
        layout.Controls.Add(buttonPanel, 0, 2);

        AcceptButton = btnOk;
        CancelButton = btnCancel;

        // ── Live-Werte ───────────────────────────────────────────────────────
        _refreshTimer = new System.Windows.Forms.Timer { Interval = 2000 };
        _refreshTimer.Tick += (_, _) => RefreshValues();
        _refreshTimer.Start();
        FormClosed += (_, _) => _refreshTimer.Stop();
    }

    private void PopulateGrid()
    {
        _grid.Rows.Clear();

        // Sensoren sortiert nach Kategorie → Typ → Name
        var sorted = _sensors
            .OrderBy(s => s.HardwareCategory)
            .ThenBy(s => s.SensorType.ToString())
            .ThenBy(s => s.SensorName)
            .ToList();

        foreach (var sensor in sorted)
        {
            var existing = _existingEntries
                .FirstOrDefault(e => e.Identifier == sensor.Identifier);

            var mqttKey = existing?.MqttKey ?? "";
            var enabled = existing?.IsEnabled ?? false;

            var valueStr = sensor.CurrentValue.HasValue
                ? $"{sensor.CurrentValue.Value:0.#} {sensor.Unit}"
                : "–";

            var idx = _grid.Rows.Add(enabled, sensor.HardwareCategory,
                sensor.TypeLabel, sensor.SensorName, mqttKey, valueStr);

            _grid.Rows[idx].Tag = sensor.Identifier;

            // Vorauswahl (Standardwerte) hell hervorheben wenn noch keine Config vorhanden
            if (enabled)
            {
                _grid.Rows[idx].DefaultCellStyle.Font =
                    new Font(_grid.Font, FontStyle.Bold);
            }
        }
    }

    private void ResetToDefaults()
    {
        var defaults = HardwareMonitorService.BuildDefaultSelection(_sensors);

        foreach (DataGridViewRow row in _grid.Rows)
        {
            var id  = (string?)row.Tag ?? "";
            var def = defaults.FirstOrDefault(d => d.Identifier == id);

            row.Cells[ColEnabled].Value = def is not null;
            if (def is not null)
            {
                row.Cells[ColMqttKey].Value = def.MqttKey;
                row.DefaultCellStyle.Font = new Font(_grid.Font, FontStyle.Bold);
            }
            else
            {
                row.DefaultCellStyle.Font = _grid.Font;
            }
        }
    }

    private void RefreshValues()
    {
        try
        {
            var values = _getLiveValues();
            _grid.SuspendLayout();
            foreach (DataGridViewRow row in _grid.Rows)
            {
                var id = (string?)row.Tag ?? "";
                var sensor = _sensors.FirstOrDefault(s => s.Identifier == id);
                if (sensor is null) continue;

                if (values.TryGetValue(id, out var val))
                    row.Cells[ColValue].Value = $"{val:0.#} {sensor.Unit}";
            }
            _grid.ResumeLayout();
        }
        catch { /* ignorieren – passiert beim Schließen */ }
    }

    private void CollectResult()
    {
        Result = [];
        foreach (DataGridViewRow row in _grid.Rows)
        {
            var enabled = row.Cells[ColEnabled].Value is true;
            var mqttKey = (row.Cells[ColMqttKey].Value as string ?? "").Trim();
            var id      = (string?)row.Tag ?? "";

            if (string.IsNullOrEmpty(id)) continue;

            // Nur aktivierte Sensoren mit einem MQTT-Schlüssel aufnehmen;
            // deaktivierte Sensoren werden trotzdem gespeichert um den Key zu merken.
            if (enabled && string.IsNullOrEmpty(mqttKey))
            {
                MessageBox.Show(
                    Localization.T("Sensors.MissingKey.Body",
                        _sensors.FirstOrDefault(s => s.Identifier == id)?.SensorName ?? ""),
                    Localization.T("Sensors.MissingKey.Title"), MessageBoxButtons.OK, MessageBoxIcon.Warning);
                DialogResult = DialogResult.None;
                return;
            }

            Result.Add(new SensorEntry
            {
                Identifier = id,
                MqttKey    = mqttKey,
                IsEnabled  = enabled
            });
        }

        // Duplikate in MQTT-Keys prüfen
        var dupes = Result
            .Where(e => e.IsEnabled && !string.IsNullOrEmpty(e.MqttKey))
            .GroupBy(e => e.MqttKey)
            .Where(g => g.Count() > 1)
            .Select(g => g.Key)
            .ToList();

        if (dupes.Count > 0)
        {
            MessageBox.Show(
                Localization.T("Sensors.DupeKeys.Body", string.Join(", ", dupes)),
                Localization.T("Sensors.DupeKeys.Title"), MessageBoxButtons.OK, MessageBoxIcon.Warning);
            DialogResult = DialogResult.None;
        }
    }
}
