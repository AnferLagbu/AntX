#ifndef _BUILTINS_H
#define _BUILTINS_H

int cmd_help(int argc, char **argv);
int cmd_cls(int argc, char **argv);
int cmd_echo(int argc, char **argv);
int cmd_exit(int argc, char **argv);

int cmd_fls(int argc, char **argv);
int cmd_fcd(int argc, char **argv);
int cmd_fpwd(int argc, char **argv);
int cmd_fcat(int argc, char **argv);
int cmd_fmk(int argc, char **argv);
int cmd_fmd(int argc, char **argv);
int cmd_frm(int argc, char **argv);
int cmd_fput(int argc, char **argv);
int cmd_fsync(int argc, char **argv);

int cmd_ilogin(int argc, char **argv);
int cmd_ilogout(int argc, char **argv);
int cmd_iwho(int argc, char **argv);
int cmd_ipasswd(int argc, char **argv);

int cmd_shost(int argc, char **argv);
int cmd_sver(int argc, char **argv);

struct builtin_cmd {
    const char *name;
    int (*func)(int argc, char **argv);
};

extern struct builtin_cmd builtins[];
int shell_is_running(void);
int execute_builtin(int argc, char **argv);

#endif
