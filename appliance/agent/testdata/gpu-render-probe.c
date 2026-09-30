// Disposable integration-test workload. No screen capture or pixel readback.
#define _GNU_SOURCE
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <gbm.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(void) {
    if (getuid() == 0) { fprintf(stderr, "Probe must run as a non-root app\n"); return 1; }
    int fd = open("/dev/dri/renderD128", O_RDWR | O_CLOEXEC);
    if (fd < 0) { perror("Open assigned render device"); return 2; }
    struct gbm_device *gbm = gbm_create_device(fd);
    if (!gbm) { fprintf(stderr, "GBM device unavailable\n"); close(fd); return 3; }
    PFNEGLGETPLATFORMDISPLAYEXTPROC platform_display =
        (PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
    EGLDisplay display = platform_display ? platform_display(EGL_PLATFORM_GBM_KHR, gbm, NULL) : EGL_NO_DISPLAY;
    EGLint major, minor, count;
    EGLConfig config;
    EGLint config_attributes[] = {EGL_SURFACE_TYPE, 0, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE};
    EGLint context_attributes[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    if (display == EGL_NO_DISPLAY || !eglInitialize(display, &major, &minor) ||
        !eglBindAPI(EGL_OPENGL_ES_API) ||
        !eglChooseConfig(display, config_attributes, &config, 1, &count) || count < 1) {
        fprintf(stderr, "Hardware EGL configuration unavailable: 0x%x\n", eglGetError()); return 4;
    }
    EGLContext context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attributes);
    if (context == EGL_NO_CONTEXT || !eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context)) {
        fprintf(stderr, "Hardware EGL context unavailable: 0x%x\n", eglGetError()); return 5;
    }
    const char *renderer = (const char *)glGetString(GL_RENDERER);
    printf("uid=%u renderer=%s version=%s\n", (unsigned)getuid(), renderer ? renderer : "unknown", glGetString(GL_VERSION));
    if (!renderer || !strstr(renderer, "virgl") || strstr(renderer, "llvmpipe") || strstr(renderer, "softpipe")) {
        fprintf(stderr, "Expected VirtIO hardware rendering, not software fallback\n"); return 6;
    }
    GLuint framebuffer, color;
    glGenFramebuffers(1, &framebuffer);
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    glGenRenderbuffers(1, &color);
    glBindRenderbuffer(GL_RENDERBUFFER, color);
    glRenderbufferStorage(GL_RENDERBUFFER, GL_RGBA4, 1, 1);
    glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, color);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) return 7;
    glViewport(0, 0, 1, 1);
    glClearColor(0.25f, 0.5f, 0.75f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    glFinish();
    if (glGetError() != GL_NO_ERROR) return 8;
    glDeleteRenderbuffers(1, &color);
    glDeleteFramebuffers(1, &framebuffer);
    eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroyContext(display, context);
    eglTerminate(display);
    gbm_device_destroy(gbm);
    close(fd);
    puts("non-root GPU workload completed");
    return 0;
}
