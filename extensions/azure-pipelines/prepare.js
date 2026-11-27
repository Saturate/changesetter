// Readies the task folders for `tfx extension create`. Azure Pipelines ships
// each task folder on its own, so code the tasks share and their npm
// dependencies have to be inside every folder. Usage: node prepare.js
const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

const shared = path.join(__dirname, "shared");
for (const task of ["ChangsetterCheck", "ChangsetterRelease"]) {
  const dir = path.join(__dirname, "tasks", task);
  const target = path.join(dir, "shared");
  fs.mkdirSync(target, { recursive: true });
  for (const file of fs.readdirSync(shared)) {
    if (file.endsWith(".js") && !file.endsWith(".test.js")) {
      fs.copyFileSync(path.join(shared, file), path.join(target, file));
    }
  }
  execFileSync("npm", ["ci", "--omit=dev", "--no-audit", "--no-fund"], { cwd: dir, stdio: "inherit", shell: process.platform === "win32" });
  console.log(`Prepared tasks/${task}`);
}
