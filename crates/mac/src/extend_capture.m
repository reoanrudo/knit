#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <VideoToolbox/VideoToolbox.h>
#import <CoreGraphics/CoreGraphics.h>
#import <stdatomic.h>

typedef bool (*KnitFrameCallback)(void *, const uint8_t *, size_t, int, const uint8_t *, size_t, uint64_t, bool);

@interface KnitCapture : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic, strong) SCStream *stream;
@property(nonatomic) VTCompressionSessionRef encoder;
@property(nonatomic) KnitFrameCallback callback;
@property(nonatomic) void *context;
@property(nonatomic) dispatch_queue_t queue;
@property(nonatomic) atomic_bool forceKey;
- (void)requestKey;
@end

static void encoded(void *refcon, void *source, OSStatus status, VTEncodeInfoFlags flags, CMSampleBufferRef sample) {
    (void)source; (void)flags;
    KnitCapture *capture = (__bridge KnitCapture *)refcon;
    if (status != noErr || !sample || !CMSampleBufferDataIsReady(sample)) return;
    CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample, false);
    CFDictionaryRef info = attachments && CFArrayGetCount(attachments) ? CFArrayGetValueAtIndex(attachments, 0) : NULL;
    bool key = !info || !CFDictionaryContainsKey(info, kCMSampleAttachmentKey_NotSync);
    NSMutableData *annex = [NSMutableData data];
    const uint8_t start[] = {0, 0, 0, 1};
    int lengthSize = 0;
    CMFormatDescriptionRef format = CMSampleBufferGetFormatDescription(sample);
    const uint8_t *parameter = NULL;
    size_t parameterSize = 0, parameterCount = 0;
    if (CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, 0, &parameter, &parameterSize, &parameterCount, &lengthSize) != noErr || lengthSize < 1 || lengthSize > 4) return;
    if (key) {
        for (size_t i = 0; i < parameterCount; i++) {
            if (CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, i, &parameter, &parameterSize, NULL, NULL) != noErr) return;
            [annex appendBytes:start length:4];
            [annex appendBytes:parameter length:parameterSize];
        }
    }
    CMBlockBufferRef block = CMSampleBufferGetDataBuffer(sample);
    if (!block) return;
    size_t size = CMBlockBufferGetDataLength(block);
    if (size > 8 * 1024 * 1024) return;
    NSMutableData *avcc = [NSMutableData dataWithLength:size];
    if (CMBlockBufferCopyDataBytes(block, 0, size, avcc.mutableBytes) != noErr) return;
    CMTime time = CMSampleBufferGetPresentationTimeStamp(sample);
    CMTime us = CMTimeConvertScale(time, 1000000, kCMTimeRoundingMethod_Default);
    if (capture.callback && capture.callback(capture.context, avcc.bytes, avcc.length, lengthSize, annex.bytes, annex.length, us.value < 0 ? 0 : (uint64_t)us.value, key)) [capture requestKey];
}

@implementation KnitCapture
- (void)requestKey { atomic_store(&_forceKey, true); }
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sample ofType:(SCStreamOutputType)type {
    (void)stream;
    if (type != SCStreamOutputTypeScreen || !CMSampleBufferIsValid(sample)) return;
    CVImageBufferRef image = CMSampleBufferGetImageBuffer(sample);
    if (!image) return;
    BOOL force = atomic_exchange(&_forceKey, false);
    CFDictionaryRef options = force ? (__bridge CFDictionaryRef)@{(__bridge NSString *)kVTEncodeFrameOptionKey_ForceKeyFrame: @YES} : NULL;
    OSStatus status = VTCompressionSessionEncodeFrame(_encoder, image, CMSampleBufferGetPresentationTimeStamp(sample), kCMTimeInvalid, options, NULL, NULL);
    if (status != noErr) NSLog(@"[extend] 動画の符号化に失敗: %d", (int)status);
}
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    (void)stream;
    NSLog(@"[extend] 画面取り込みが停止: %@", error);
}
@end

// 戻り値は ARC の所有権を呼び出し元に渡す。
void *knit_capture_start(uint32_t displayID, uint32_t width, uint32_t height, uint32_t fps, uint32_t bitrate, KnitFrameCallback callback, void *context) {
    @autoreleasepool {
        if (@available(macOS 12.3, *)) {} else {
            fprintf(stderr, "[extend] macOS 12.3 以降が必要です\n");
            return NULL;
        }
        if (!CGPreflightScreenCaptureAccess()) {
            fprintf(stderr, "[extend] システム設定 › プライバシーとセキュリティ › 画面収録 で Knit をオンにしてください\n");
            return NULL;
        }
        KnitCapture *capture = [KnitCapture new];
        capture.callback = callback; capture.context = context;
        capture.queue = dispatch_queue_create("jp.knit.extend.capture", DISPATCH_QUEUE_SERIAL);
        VTCompressionSessionRef encoder = NULL;
        OSStatus status = VTCompressionSessionCreate(NULL, (int32_t)width, (int32_t)height, kCMVideoCodecType_H264, NULL, NULL, NULL, encoded, (__bridge void *)capture, &encoder);
        if (status != noErr || !encoder) return NULL;
        capture.encoder = encoder;
        if (VTSessionSetProperty(encoder, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration, (__bridge CFTypeRef)@2) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AverageBitRate, (__bridge CFTypeRef)@(bitrate)) != noErr ||
            VTCompressionSessionPrepareToEncodeFrames(encoder) != noErr) {
            NSLog(@"[extend] H.264 符号化の設定に失敗しました");
            VTCompressionSessionInvalidate(encoder); CFRelease(encoder); return NULL;
        }
        dispatch_semaphore_t sem = dispatch_semaphore_create(0);
        __block SCShareableContent *content = nil;
        [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent *value, NSError *error) {
            if (error) NSLog(@"[extend] 画面一覧の取得に失敗: %@", error);
            content = value; dispatch_semaphore_signal(sem);
        }];
        dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
        SCDisplay *display = nil;
        for (SCDisplay *candidate in content.displays) if (candidate.displayID == displayID) { display = candidate; break; }
        if (!display) { VTCompressionSessionInvalidate(encoder); CFRelease(encoder); return NULL; }
        SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
        SCStreamConfiguration *config = [SCStreamConfiguration new];
        config.width = width; config.height = height;
        config.minimumFrameInterval = CMTimeMake(1, (int32_t)fps);
        config.queueDepth = 3;
        capture.stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:capture];
        NSError *error = nil;
        if (![capture.stream addStreamOutput:capture type:SCStreamOutputTypeScreen sampleHandlerQueue:capture.queue error:&error]) {
            NSLog(@"[extend] 出力の追加に失敗: %@", error);
            VTCompressionSessionInvalidate(encoder); CFRelease(encoder); return NULL;
        }
        __block NSError *startError = nil;
        [capture.stream startCaptureWithCompletionHandler:^(NSError *e) { startError = e; dispatch_semaphore_signal(sem); }];
        dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
        if (startError) { NSLog(@"[extend] 取り込み開始に失敗: %@", startError); VTCompressionSessionInvalidate(encoder); CFRelease(encoder); return NULL; }
        return (__bridge_retained void *)capture;
    }
}
void knit_capture_force_key(void *ptr) { KnitCapture *capture = (__bridge KnitCapture *)ptr; [capture requestKey]; }
void knit_capture_stop(void *ptr) {
    if (!ptr) return;
    KnitCapture *capture = (__bridge_transfer KnitCapture *)ptr;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [capture.stream stopCaptureWithCompletionHandler:^(NSError *error) { if (error) NSLog(@"[extend] 取り込み停止に失敗: %@", error); dispatch_semaphore_signal(sem); }];
    dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
    dispatch_sync(capture.queue, ^{});
    NSError *removeError = nil;
    if (![capture.stream removeStreamOutput:capture type:SCStreamOutputTypeScreen error:&removeError]) {
        NSLog(@"[extend] 出力の解除に失敗: %@", removeError);
    }
    capture.stream = nil;
    VTCompressionSessionCompleteFrames(capture.encoder, kCMTimeInvalid);
    VTCompressionSessionInvalidate(capture.encoder);
    CFRelease(capture.encoder);
}
