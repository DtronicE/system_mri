# Clean-room policy

System MRI re-implements *concepts*, not code, from `htop`, `strace`, `lsof`,
`netstat`, `pmap` and `perf`.

* Allowed sources: kernel documentation (`Documentation/filesystems/proc.rst`),
  man pages (`proc(5)`, `proc_pid_*(5)`, `ptrace(2)`, `perf_event_open(2)`,
  `waitpid(2)`), kernel UAPI headers, and observed behaviour of the tools.
* Not used: the source code of those tools. Contributors who have read it
  closely should say so in the PR so a reviewer can check for similarity.
* Format knowledge (e.g. the layout of `/proc/net/tcp`) comes from documentation
  and experiments on a live kernel. Every parser has a fixture test that
  records the format assumption.
