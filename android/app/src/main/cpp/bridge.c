#include <jni.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
extern char *knit_android_call(int, uint64_t, const char *, void *, bool (*)(void *, const char *, const char *));
extern void knit_android_free(char *);
struct save_context { JNIEnv *env; jobject store; };
static bool save_credential(void *raw, const char *token, const char *address) {
    struct save_context *ctx = raw;
    JNIEnv *env = ctx->env;
    jclass cls = (*env)->GetObjectClass(env, ctx->store);
    jmethodID method = (*env)->GetMethodID(env, cls, "save", "(Ljava/lang/String;Ljava/lang/String;)Z");
    if (!method) { (*env)->ExceptionClear(env); return false; }
    jstring t = (*env)->NewStringUTF(env, token);
    jstring a = (*env)->NewStringUTF(env, address);
    bool ok = (*env)->CallBooleanMethod(env, ctx->store, method, t, a);
    if ((*env)->ExceptionCheck(env)) { (*env)->ExceptionClear(env); ok = false; }
    (*env)->DeleteLocalRef(env, t); (*env)->DeleteLocalRef(env, a); (*env)->DeleteLocalRef(env, cls);
    return ok;
}
JNIEXPORT jbyteArray JNICALL Java_app_knit_Native_call(JNIEnv *env, jclass cls, jint op, jlong handle, jbyteArray args, jobject store) {
    (void)cls;
    jsize size = (*env)->GetArrayLength(env, args);
    if (size > 2*1024*1024) return NULL;
    char *input = malloc((size_t)size + 1);
    if (!input) return NULL;
    (*env)->GetByteArrayRegion(env,args,0,size,(jbyte *)input); input[size] = 0;
    struct save_context ctx = {env,store};
    char *output = knit_android_call(op,(uint64_t)handle,input,&ctx,store ? save_credential : NULL);
    free(input);
    if (!output) return NULL;
    size_t len = strlen(output);
    jbyteArray result = (*env)->NewByteArray(env, (jsize)len);
    if (result) (*env)->SetByteArrayRegion(env,result,0,(jsize)len,(const jbyte *)output);
    knit_android_free(output);
    return result;
}
