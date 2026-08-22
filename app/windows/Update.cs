// Checking for an update from the window, so the signed-release mechanism is
// reachable by somebody who has only ever opened the app. The Mac has had this
// in its menu bar since the mechanism existed; Windows did not, which meant an
// update could be published and the person it was for would never learn of it
// without a terminal.
//
// The work is all in `collab update`; this asks, shows what it said, and asks
// before installing. -json so the answer is parsed rather than scraped.
using System;
using System.Text.Json;
using System.Windows;

namespace Collab
{
    internal static class Updater
    {
        public static void CheckForUpdates(Window owner)
        {
            string raw = Core.Run("update -json");
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

            var result = Core.Run("update -yes");
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
