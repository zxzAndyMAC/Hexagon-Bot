// QA16 real exec regression: uses the shipped adapter and kernel Seatbelt.
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdlib.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/socket.h>
#include <arpa/inet.h>
#include <sys/syscall.h>
extern char **environ;
int main(int argc, char **argv) {
  if(argc>1) {
    assert(argc==6);
    int fd=atoi(argv[1]), inherited=atoi(argv[2]);
    assert((fcntl(fd,F_GETFD)>=0)==inherited);
    assert(getpgrp()==atoi(argv[3]));
    assert(setsid()==-1); assert(setpgid(0,0)==-1);
    assert(open("allowed/.env",O_RDONLY)==-1);
    assert(open("allowed/opaque.data",O_RDONLY)==-1);
    assert(open("allowed/alias.data",O_RDONLY)==-1);
    assert(open(argv[4],O_WRONLY|O_CREAT,0600)==-1);
    int sock=socket(AF_INET,SOCK_STREAM,0); assert(sock>=0);
    struct sockaddr_in addr={.sin_family=AF_INET,.sin_port=htons(atoi(argv[5]))};
    assert(inet_pton(AF_INET,"127.0.0.1",&addr.sin_addr)==1);
    assert(connect(sock,(struct sockaddr *)&addr,sizeof(addr))==-1); close(sock);
    errno=0;
    assert(syscall(SYS_posix_spawn,NULL,NULL,NULL,NULL,NULL,NULL)==-1 && errno==78);
    return 0;
  }
  const char *outside=getenv("QA_OUTSIDE"); const char *port=getenv("QA_PORT");
  assert(outside && port);
  for(int original_cloexec=0; original_cloexec<2; original_cloexec++) {
    int pipefd[2]; assert(pipe(pipefd)==0);
    assert(fcntl(pipefd[0],F_SETFD,original_cloexec?FD_CLOEXEC:0)==0);
    posix_spawnattr_t attr; assert(posix_spawnattr_init(&attr)==0);
    assert(posix_spawnattr_setflags(&attr,POSIX_SPAWN_CLOEXEC_DEFAULT)==0);
    posix_spawn_file_actions_t acts; assert(posix_spawn_file_actions_init(&acts)==0);
    assert(posix_spawn_file_actions_adddup2(&acts,pipefd[0],pipefd[0])==0);
    char fd[24], expect[8], group[24];
    snprintf(fd,sizeof(fd),"%d",pipefd[0]); snprintf(expect,sizeof(expect),"%d",!original_cloexec);
    snprintf(group,sizeof(group),"%d",getpgrp());
    char *args[]={argv[0],fd,expect,group,(char *)outside,(char *)port,NULL};
    pid_t child; assert(posix_spawn(&child,argv[0],&acts,&attr,args,environ)==0);
    int status; assert(waitpid(child,&status,0)==child && WIFEXITED(status) && WEXITSTATUS(status)==0);
    assert(posix_spawnattr_setflags(&attr,POSIX_SPAWN_SETPGROUP)==0);
    assert(posix_spawn(&child,argv[0],&acts,&attr,args,environ)==ENOTSUP);
    assert(posix_spawnattr_setflags(&attr,POSIX_SPAWN_SETSID)==0);
    assert(posix_spawn(&child,argv[0],&acts,&attr,args,environ)==ENOTSUP);
    assert(posix_spawn_file_actions_addopen(&acts,4,"ignored",O_RDONLY,0)==ENOTSUP);
    assert(posix_spawn_file_actions_destroy(&acts)==0);
    assert(posix_spawnattr_destroy(&attr)==0); close(pipefd[0]); close(pipefd[1]);
  }
  puts("FIREFOX_FORK_BOUNDARIES_OK");
  return 0;
}
