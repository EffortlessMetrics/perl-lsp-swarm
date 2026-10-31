// Retired local publisher (#16802); argv and ambient credentials grant no authority.
process.stderr.write(
    'Direct local extension publication is disabled. Follow the reviewed release transaction (#13768 / #6055); see docs/release/RUNBOOK.md.\n',
);
process.exitCode = 1;
