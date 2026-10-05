// QA16 / 2026-10-05: Firefox has no fork fallback when Seatbelt denies
// posix_spawn (XNU can otherwise create a new session inside spawn). Keep the
// kernel fence; implement only Firefox's observed absolute-path launch subset.
// Unknown flags/actions fail closed: an extra intervention is cheaper than an
// uncontained browser descendant. All children inherit the outer Seatbelt.
#include <spawn.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <sys/resource.h>
#include <signal.h>
#include <limits.h>

enum kind { DUP, CLOSE, INHERIT, CHDIR };
struct action { enum kind kind; int a, b; char path[PATH_MAX]; };
struct actions { void *key; int count; struct action items[64]; struct actions *next; };
static struct actions *lists;
static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static struct actions *find(void *key) {
  for (struct actions *x = lists; x; x=x->next) if (x->key==key) return x;
  return NULL;
}
static int record(posix_spawn_file_actions_t *key, enum kind kind, int a, int b, const char *path) {
  pthread_mutex_lock(&mutex);
  struct actions *x=find(key);
  if (!x || x->count>=64 || (path && strlen(path)>=PATH_MAX)) { pthread_mutex_unlock(&mutex); return ENOTSUP; }
  struct action *act=&x->items[x->count++];
  act->kind=kind; act->a=a; act->b=b;
  if (path) strcpy(act->path,path);
  pthread_mutex_unlock(&mutex); return 0;
}
static int hx_init(posix_spawn_file_actions_t *key) {
  int rc=posix_spawn_file_actions_init(key); if (rc) return rc;
  struct actions *x=calloc(1,sizeof(*x)); if (!x) { posix_spawn_file_actions_destroy(key); return ENOMEM; }
  x->key=key;
  pthread_mutex_lock(&mutex); x->next=lists; lists=x; pthread_mutex_unlock(&mutex); return 0;
}
static int hx_destroy(posix_spawn_file_actions_t *key) {
  pthread_mutex_lock(&mutex);
  struct actions **x=&lists; while (*x && (*x)->key!=key) x=&(*x)->next;
  if (*x) { struct actions *old=*x; *x=old->next; free(old); }
  pthread_mutex_unlock(&mutex);
  return posix_spawn_file_actions_destroy(key);
}
// Preserve libc validation/allocation. Track the container address because libc
// may replace its opaque allocation when appending actions.
static int hx_dup(posix_spawn_file_actions_t *key,int a,int b) { int rc=posix_spawn_file_actions_adddup2(key,a,b); return rc ? rc : record(key,DUP,a,b,NULL); }
static int hx_close(posix_spawn_file_actions_t *key,int a) { int rc=posix_spawn_file_actions_addclose(key,a); return rc ? rc : record(key,CLOSE,a,0,NULL); }
static int hx_inherit(posix_spawn_file_actions_t *key,int a) { int rc=posix_spawn_file_actions_addinherit_np(key,a); return rc ? rc : record(key,INHERIT,a,0,NULL); }
static int hx_chdir(posix_spawn_file_actions_t *key,const char *path) { int rc=posix_spawn_file_actions_addchdir_np(key,path); return rc ? rc : record(key,CHDIR,0,0,path); }
static int hx_open(posix_spawn_file_actions_t *key,int fd,const char *path,int flags,mode_t mode) { (void)key;(void)fd;(void)path;(void)flags;(void)mode; return ENOTSUP; }
static int hx_fchdir(posix_spawn_file_actions_t *key,int fd) { (void)key;(void)fd; return ENOTSUP; }

static int hx_spawn(pid_t *pid,const char *path,const posix_spawn_file_actions_t *key,
                    const posix_spawnattr_t *attr,char *const argv[],char *const env[]) {
  if (!pid || !path || path[0]!='/') return ENOTSUP;
  short flags=0;
  if (attr && posix_spawnattr_getflags(attr,&flags)) return ENOTSUP;
  if (flags & ~(POSIX_SPAWN_CLOEXEC_DEFAULT|POSIX_SPAWN_SETSIGMASK|POSIX_SPAWN_SETSIGDEF)) return ENOTSUP;
  sigset_t mask, defaults;
  if ((flags&POSIX_SPAWN_SETSIGMASK) && posix_spawnattr_getsigmask(attr,&mask)) return ENOTSUP;
  if ((flags&POSIX_SPAWN_SETSIGDEF) && posix_spawnattr_getsigdefault(attr,&defaults)) return ENOTSUP;
  struct actions copy; memset(&copy,0,sizeof(copy));
  if (key) {
    pthread_mutex_lock(&mutex); struct actions *x=find((void *)key);
    if (!x) { pthread_mutex_unlock(&mutex); return ENOTSUP; }
    memcpy(&copy,x,sizeof(copy)); pthread_mutex_unlock(&mutex);
  }
  struct rlimit limit; if (getrlimit(RLIMIT_NOFILE,&limit) || limit.rlim_cur>1048576) return ENOTSUP;
  int error_pipe[2]; if (pipe(error_pipe)) return errno;
  if (fcntl(error_pipe[1],F_SETFD,FD_CLOEXEC)==-1) { int e=errno; close(error_pipe[0]);close(error_pipe[1]);return e; }
  // Reserve the handshake fd above every action fd so dup/close cannot clobber it.
  int minimum=3;
  for (int i=0;i<copy.count;i++) {
    struct action *a=&copy.items[i];
    if (a->kind!=CHDIR) {
      if (a->a==INT_MAX || (a->kind==DUP && a->b==INT_MAX)) { close(error_pipe[0]); close(error_pipe[1]); return ENOTSUP; }
      if (a->a>=minimum) minimum=a->a+1; if (a->kind==DUP && a->b>=minimum) minimum=a->b+1;
    }
  }
  int channel=fcntl(error_pipe[1],F_DUPFD_CLOEXEC,minimum);
  close(error_pipe[1]); if(channel<0) { int e=errno;close(error_pipe[0]);return e; }
  pid_t child=fork();
  if(child<0) { int e=errno;close(channel);close(error_pipe[0]);return e; }
  if(child==0) {
    close(error_pipe[0]); int failure=0;
    for(int i=0;i<copy.count;i++) {
      struct action *a=&copy.items[i]; int rc=0;
      switch(a->kind) {
        case DUP: rc=dup2(a->a,a->b); break;
        case CLOSE: rc=close(a->a); break;
        case INHERIT: rc=fcntl(a->a,F_GETFD); break;
        case CHDIR: rc=chdir(a->path); break;
      }
      if(rc<0) { failure=errno;goto failed; }
    }
    // QA16 reviewer / XNU exec_handle_file_actions + fdt_exec: DEFAULT adds
    // close-on-exec only to descriptors not explicitly inherited. Setting it
    // before dup2(fd,fd) incorrectly closed originally non-CLOEXEC descriptors;
    // indiscriminately clearing it would retain originally CLOEXEC descriptors.
    if(flags&POSIX_SPAWN_CLOEXEC_DEFAULT) {
      for(int fd=0;fd<(int)limit.rlim_cur;fd++) if(fd!=channel) {
        int value=fcntl(fd,F_GETFD); if(value<0) continue;
        int retained=0;
        for(int i=0;i<copy.count;i++) {
          struct action *a=&copy.items[i];
          if((a->kind==DUP && a->b==fd) || (a->kind==INHERIT && a->a==fd)) retained=1;
          if(a->kind==CLOSE && a->a==fd) retained=0;
        }
        if(!retained && fcntl(fd,F_SETFD,value|FD_CLOEXEC)<0) { failure=errno;goto failed; }
      }
    }
    if((flags&POSIX_SPAWN_SETSIGMASK) && sigprocmask(SIG_SETMASK,&mask,NULL)<0) { failure=errno;goto failed; }
    if(flags&POSIX_SPAWN_SETSIGDEF) for(int sig=1;sig<NSIG;sig++) if(sigismember(&defaults,sig)==1) {
      struct sigaction action; memset(&action,0,sizeof(action)); action.sa_handler=SIG_DFL; sigemptyset(&action.sa_mask);
      if(sigaction(sig,&action,NULL)<0) { failure=errno;goto failed; }
    }
    execve(path,argv,env); failure=errno;
failed:
    while(write(channel,&failure,sizeof(failure))<0 && errno==EINTR) {}
    _exit(127);
  }
  close(channel); int error=0; ssize_t count; size_t received=0;
  do {
    count=read(error_pipe[0],(char *)&error+received,sizeof(error)-received);
    if(count>0) received+=(size_t)count;
  } while((count<0 && errno==EINTR) || (count>0 && received<sizeof(error)));
  close(error_pipe[0]);
  if(count<0 || received!=0) { kill(child,SIGKILL);while(waitpid(child,NULL,0)<0 && errno==EINTR){}; return received==sizeof(error) && error ? error : EIO; }
  *pid=child; return 0;
}
#define INTERPOSE(replacement,original) \
  __attribute__((used)) static const struct { const void *replace; const void *orig; } \
  interpose_##original __attribute__((section("__DATA,__interpose"))) = {(const void *)&replacement,(const void *)&original};
INTERPOSE(hx_init,posix_spawn_file_actions_init)
INTERPOSE(hx_destroy,posix_spawn_file_actions_destroy)
INTERPOSE(hx_dup,posix_spawn_file_actions_adddup2)
INTERPOSE(hx_close,posix_spawn_file_actions_addclose)
INTERPOSE(hx_inherit,posix_spawn_file_actions_addinherit_np)
INTERPOSE(hx_chdir,posix_spawn_file_actions_addchdir_np)
INTERPOSE(hx_chdir,posix_spawn_file_actions_addchdir)
INTERPOSE(hx_fchdir,posix_spawn_file_actions_addfchdir_np)
INTERPOSE(hx_fchdir,posix_spawn_file_actions_addfchdir)
INTERPOSE(hx_open,posix_spawn_file_actions_addopen)
INTERPOSE(hx_spawn,posix_spawnp)
INTERPOSE(hx_spawn,posix_spawn)
