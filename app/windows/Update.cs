// Checking for an update from the window, so the signed-release mechanism is
// reachable by somebody who has only ever opened the app. The Mac has had this
// in its menu bar since the mechanism existed; Windows did not, which meant an
// update could be published and the person it was for would never learn of it
// without a terminal.
//
// The work is all in `collab update`; this asks, shows what it said, and asks
// before installing. -json so the answer is parsed rather than scraped.
using System;
using System.Threading.Tasks;
using System.Text.Json;
using System.Windows;
using System.Windows.Input;

namespace Collab
{
    internal static class Updater
    {
        /// Both halves of this run off the window's thread.
        ///
        /// Checking dials GitHub; installing downloads about 160 MB and replaces
        /// three files. Doing either on the UI thread means the window has
        /// nothing to draw with until it finishes — Tankun saw the check as a
        /// pause and the install as a hang, and a hang is exactly what it was.
        ///
        /// It looked like a crash before this app replaced the notifier, because
        /// the running executable was being swapped underneath itself.
        public static async void CheckForUpdates(Window owner)
        {
            var busy = new Cursor[] { owner.Cursor };
            owner.Cursor = Cursors.AppStarting;
            try
            {
                await CheckForUpdatesCore(owner);
            }
            finally
            {
                owner.Cursor = busy[0];
            }
        }

        static async Task CheckForUpdatesCore(Window owner)
        {
            string raw = await Task.Run(() => Core.Run("update -json"));
            JsonElement j;
            try { j = JsonDocument.Parse(raw).RootElement; }
            catch
            {
                Show(owner, "Could not check", raw.Split('\n')[0].Trim(), MessageBoxButton.OK);
                return;
            }

            bool ok = j.TryGetProperty("ok", out var o) && o.GetBoolean();
            if (!ok)
            {
                var err = j.TryGetProperty("error", out var e) ? e.GetString() : null;
                Show(owner, "Could not check", err ?? "Unknown problem.", MessageBoxButton.OK);
                return;
            }

            string current = Str(j, "current") ?? "?";
            string available = Str(j, "available") ?? "?";
            bool newer = j.TryGetProperty("newer", out var n) && n.GetBoolean();

            if (!newer)
            {
                Show(owner, "Up to date",
                     $"You are running {current}, which is the latest signed release.",
                     MessageBoxButton.OK);
                return;
            }

            var notes = Str(j, "notes");
            var body = $"You are running {current}. {available} is available."
                     + (string.IsNullOrWhiteSpace(notes) ? "" : "\n\n" + notes)
                     + "\n\nIt is checked against the project's signing key before anything is "
                     + "replaced, and refused if it does not match.";
            if (Show(owner, $"Update to {available}?", body, MessageBoxButton.OKCancel) != MessageBoxResult.OK)
                return;

            // The long one: a download and three file replacements. The window
            // keeps painting while it runs, which is the whole point.
            var result = await Task.Run(() => Core.Run("update -yes"));
            var last = result.Trim().Split('\n');
            Show(owner, "Update",
                 string.Join("\n", last.Length > 6 ? last[^6..] : last).Trim(),
                 MessageBoxButton.OK);
        }

        static string? Str(JsonElement j, string name) =>
            j.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

        static MessageBoxResult Show(Window owner, string title, string body, MessageBoxButton buttons) =>
            MessageBox.Show(owner, body, title, buttons, MessageBoxImage.Information);
    }
}
