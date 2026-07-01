using System.Threading;
using System.Windows.Forms;

namespace PCBridge;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        Application.SetHighDpiMode(HighDpiMode.SystemAware);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        using var mutex = new Mutex(
            initiallyOwned: true,
            name: "PCBridge-SingleInstance-9F3B1C2A",
            createdNew: out bool isNew);

        if (!isNew)
        {
            MessageBox.Show(
                "PCBridge läuft bereits im Hintergrund.\nSchau im Systray nach dem Symbol.",
                "PCBridge", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        Application.Run(new TrayAppContext());
    }
}
