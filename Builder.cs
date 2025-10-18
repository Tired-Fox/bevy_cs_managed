using System.Text;
using System.Text.Json;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.MSBuild;
using Microsoft.Build.Locator;
using System.Diagnostics;

record BuildRequest(string ProjectFile, string OutFile);
record BuildDiagnostic(string Filename, int Line, int Column, string Severity, string Code, string Message);
record BuildResult(bool Success, double ElapsedMs, BuildDiagnostic[] Diagnostics);

class Program
{
    static async Task Main()
    {
        Console.InputEncoding = Encoding.UTF8;
        Console.OutputEncoding = Encoding.UTF8;

        // Register the MSBuild toolset for the selected SDK version
        // This automatically uses the MSBuild that ships with the current runtime.
        MSBuildLocator.RegisterDefaults();

        using var workspace = MSBuildWorkspace.Create();

        string? line;
        while ((line = Console.ReadLine()) != null)
        {
            try
            {
                var req = JsonSerializer.Deserialize<BuildRequest>(line)!;

                var sw = Stopwatch.StartNew();
                var project = await workspace.OpenProjectAsync(req.ProjectFile);
                var compilation = await project.GetCompilationAsync();

                // Emit assembly to disk
                var emitResult = compilation!.Emit(req.OutFile);

                var diagnostics = compilation!.GetDiagnostics();
                bool success = emitResult.Success;
                var messages = new List<BuildDiagnostic>();

                foreach (var d in diagnostics)
					if (d.Severity != DiagnosticSeverity.Hidden) {
						var span = d.Location.GetLineSpan();
						messages.Add(new BuildDiagnostic(
							span.Path,
							span.StartLinePosition.Line + 1,
							span.StartLinePosition.Character + 1,
							d.Severity.ToString(),
							d.Id,
							d.GetMessage()
						));
					}

                sw.Stop();
                var result = new BuildResult(success, double.Round(sw.Elapsed.TotalMilliseconds), messages.ToArray());
                Console.WriteLine(JsonSerializer.Serialize(result));
            }
            catch
            {
                var result = new BuildResult(false, 0, []);
                Console.WriteLine(JsonSerializer.Serialize(result));
            }
        }
    }
}
