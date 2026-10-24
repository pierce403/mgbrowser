// Keep the executed browser arguments and recorded scripting policy together.
export function browserConfiguration(fixture) {
  return {
    args: ['--remote-debugging-port=0', '--no-auto-update', '--disable-scripts', fixture],
    scriptsEnabled: false,
  };
}
