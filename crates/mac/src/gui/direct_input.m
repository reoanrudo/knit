#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <Carbon/Carbon.h>
#include <stdint.h>
#include <stdbool.h>
typedef void (*Commit)(uint64_t, const uint8_t *, size_t);
typedef void (*EditingKey)(uint64_t, uint16_t, uint64_t);

static void selectMode(NSTextInputContext *context, uint16_t kc) {
    NSString *match = nil;
    for (NSString *source in context.keyboardInputSources) {
        if (kc == 104 && [source hasSuffix:@"Japanese"]) match = source;
        if (kc == 104 && ([source hasSuffix:@"Hiragana"] || [source isEqual:@"com.google.inputmethod.Japanese.base"])) { match = source; break; }
        if (kc == 102 && ([source hasSuffix:@".ABC"] || [source hasSuffix:@".US"] || [source hasSuffix:@".Roman"])) { match = source; break; }
    }
    if (match) context.selectedKeyboardInputSource = match;
}

// NSTextView owns the NSTextInputContext, marked range and candidate placement.
// Only insertText/unmarkText commits leave this process. Preedit stays local.
@interface KnitDirectTextView : NSTextView
@property uint64_t epoch;
@property Commit commit;
@property EditingKey key;
@property BOOL suppress;
@property BOOL inserting;
@property NSEventModifierFlags eventFlags;
- (void)discard;
@end

@implementation KnitDirectTextView
- (void)keyDown:(NSEvent *)event {
    if (event.keyCode == 102 || event.keyCode == 104) {
        if (self.hasMarkedText) [self unmarkText];
        selectMode(self.inputContext, event.keyCode);
        return;
    }
    self.eventFlags = event.modifierFlags;
    [super keyDown:event];
}
- (void)sendText:(NSString *)text {
    NSData *utf8 = [text dataUsingEncoding:NSUTF8StringEncoding];
    if (!self.suppress && self.epoch && self.commit && utf8.length && utf8.length <= 1024 * 1024)
        self.commit(self.epoch, utf8.bytes, utf8.length);
}
- (void)insertText:(id)value replacementRange:(NSRange)range {
    NSString *text = [value isKindOfClass:NSAttributedString.class] ? [value string] : value;
    BOOL outerInsert = self.inserting;
    self.inserting = YES;
    [super insertText:value replacementRange:range];
    self.inserting = outerInsert;
    if (!outerInsert) {
        [self sendText:text];
        if (!self.hasMarkedText) self.string = @"";
    }
}
- (void)unmarkText {
    if (self.inserting) { [super unmarkText]; return; }
    NSString *text = self.hasMarkedText ? [self.string substringWithRange:self.markedRange] : @"";
    self.inserting = YES;
    [super unmarkText];
    self.inserting = NO;
    [self sendText:text];
    self.string = @"";
}
- (void)discard {
    self.suppress = YES;
    [self.inputContext discardMarkedText];
    [super unmarkText];
    self.string = @"";
    self.suppress = NO;
}
- (void)doCommandBySelector:(SEL)command {
    if (self.hasMarkedText) {
        if (command == @selector(cancelOperation:)) [self discard];
        else [super doCommandBySelector:command];
        return;
    }
    NSString *name = NSStringFromSelector(command);
    uint16_t kc = UINT16_MAX;
    if ([name hasPrefix:@"insertNewline"] || [name isEqual:@"insertLineBreak:"]) kc = 36;
    else if ([name hasPrefix:@"insertTab"] || [name isEqual:@"insertBacktab:"]) kc = 48;
    else if ([name hasPrefix:@"deleteBackward"] || [name isEqual:@"deleteWordBackward:"]) kc = 51;
    else if ([name hasPrefix:@"deleteForward"] || [name isEqual:@"deleteWordForward:"]) kc = 117;
    else if ([name hasPrefix:@"moveLeft"] || [name hasPrefix:@"moveBackward"]) kc = 123;
    else if ([name hasPrefix:@"moveRight"] || [name hasPrefix:@"moveForward"]) kc = 124;
    else if ([name hasPrefix:@"moveUp"]) kc = 126;
    else if ([name hasPrefix:@"moveDown"]) kc = 125;
    else if ([name hasPrefix:@"moveToBeginning"]) kc = 115;
    else if ([name hasPrefix:@"moveToEnd"]) kc = 119;
    else if ([name hasPrefix:@"pageUp"]) kc = 116;
    else if ([name hasPrefix:@"pageDown"]) kc = 121;
    else if ([name isEqual:@"cancelOperation:"]) kc = 53;
    if (kc != UINT16_MAX && !self.suppress && self.epoch && self.key)
        self.key(self.epoch, kc, self.eventFlags);
    // Never edit/retain a local committed document or accidentally invoke an
    // application command. Unknown commands are ignored.
}
@end

static NSPanel *panel;
static KnitDirectTextView *editor;
static NSRunningApplication *previousApp;

void knit_direct_input_start(uint64_t epoch, Commit commit, EditingKey key) {
    NSCAssert(NSThread.isMainThread, @"AppKit must run on the main thread");
    NSString *source = NSTextInputContext.currentInputContext.selectedKeyboardInputSource;
    // Preserve the source selected in the previous application, even when it
    // has no Cocoa input context (e.g. a browser or Electron application).
    if (!source) {
        TISInputSourceRef current = TISCopyCurrentKeyboardInputSource();
        if (current) {
            source = [(__bridge NSString *)TISGetInputSourceProperty(current, kTISPropertyInputSourceID) copy];
            CFRelease(current);
        }
    }
    if (!panel) {
        NSRect screen = NSScreen.mainScreen.visibleFrame;
        NSRect frame = NSMakeRect(NSMidX(screen) - 210, NSMinY(screen) + 24, 420, 58);
        panel = [[NSPanel alloc] initWithContentRect:frame styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
        panel.title = @"Knit · タブレットに入力（Spaceで変換・Enterで確定）";
        panel.floatingPanel = YES;
        panel.hidesOnDeactivate = NO;
        panel.releasedWhenClosed = NO;
        editor = [[KnitDirectTextView alloc] initWithFrame:NSMakeRect(0, 0, 420, 58)];
        editor.richText = NO;
        editor.allowsUndo = NO;
        editor.font = [NSFont systemFontOfSize:20];
        editor.textContainerInset = NSMakeSize(12, 12);
        editor.automaticQuoteSubstitutionEnabled = NO;
        editor.automaticDashSubstitutionEnabled = NO;
        editor.automaticTextReplacementEnabled = NO;
        editor.automaticSpellingCorrectionEnabled = NO;
        editor.automaticLinkDetectionEnabled = NO;
        panel.contentView = editor;
    }
    if (!previousApp) previousApp = NSWorkspace.sharedWorkspace.frontmostApplication;
    editor.epoch = 0;
    [editor discard];
    editor.epoch = epoch;
    editor.commit = commit;
    editor.key = key;
    [NSApp activateIgnoringOtherApps:YES];
    [panel makeKeyAndOrderFront:nil];
    [panel makeFirstResponder:editor];
    [editor.inputContext activate];
    if (source) editor.inputContext.selectedKeyboardInputSource = source;
}

bool knit_direct_input_ready(void) {
    return NSThread.isMainThread && editor.epoch && NSApp.active && panel.keyWindow && panel.firstResponder == editor;
}
bool knit_direct_input_composing(void) {
    return knit_direct_input_ready() && editor.hasMarkedText;
}
void knit_direct_input_finish(void) {
    if (knit_direct_input_composing()) [editor unmarkText];
}

void knit_direct_input_stop(void) {
    NSCAssert(NSThread.isMainThread, @"AppKit must run on the main thread");
    editor.epoch = 0; // Cancel before any IME callbacks or restoring focus.
    [editor discard];
    [editor.inputContext deactivate];
    [panel orderOut:nil];
    if (NSApp.active && previousApp && previousApp.processIdentifier != NSProcessInfo.processInfo.processIdentifier)
        [previousApp activateWithOptions:0];
    previousApp = nil;
}

void knit_direct_input_event(CGEventRef cg) {
    NSCAssert(NSThread.isMainThread, @"AppKit must run on the main thread");
    if (!editor.epoch) return;
    NSEvent *event = [NSEvent eventWithCGEvent:cg];
    if (!event) return;
    if (event.type == NSEventTypeKeyDown) {
        event = [NSEvent keyEventWithType:event.type location:NSZeroPoint modifierFlags:event.modifierFlags timestamp:event.timestamp windowNumber:panel.windowNumber context:nil characters:event.characters ?: @"" charactersIgnoringModifiers:event.charactersIgnoringModifiers ?: @"" isARepeat:event.isARepeat keyCode:event.keyCode];
        // Startup queue only. Normal hardware events pass directly to AppKit.
        [NSApp sendEvent:event];
    } else if (event.type == NSEventTypeFlagsChanged) {
        [editor flagsChanged:event];
    }
}

// An owned, offscreen NSTextView exercises the actual native callback path;
// this probe does not post OS input events, change permissions or connect.
static NSMutableArray<NSString *> *probeText;
static NSMutableArray<NSNumber *> *probeKeys;
static void probeCommit(uint64_t epoch, const uint8_t *bytes, size_t len) {
    NSCAssert(epoch == 77, @"wrong epoch");
    [probeText addObject:[[NSString alloc] initWithBytes:bytes length:len encoding:NSUTF8StringEncoding]];
}
static void probeKey(uint64_t epoch, uint16_t kc, uint64_t flags) {
    (void)flags;
    NSCAssert(epoch == 77, @"wrong epoch");
    [probeKeys addObject:@(kc)];
}
bool knit_direct_input_probe(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        probeText = [NSMutableArray array]; probeKeys = [NSMutableArray array];
        KnitDirectTextView *v = [[KnitDirectTextView alloc] initWithFrame:NSMakeRect(0,0,420,58)];
        v.epoch = 77; v.commit = probeCommit; v.key = probeKey;
        [v setMarkedText:@"にほ" selectedRange:NSMakeRange(2,0) replacementRange:NSMakeRange(NSNotFound,0)];
        [v setMarkedText:@"にほん" selectedRange:NSMakeRange(3,0) replacementRange:NSMakeRange(NSNotFound,0)];
        if (!v.hasMarkedText || probeText.count) { NSLog(@"[probe] preedit failed: %@ %@", v.string, probeText); return false; }
        [v insertText:[[NSAttributedString alloc] initWithString:@"日本"] replacementRange:NSMakeRange(NSNotFound,0)];
        if (v.hasMarkedText || ![v.string isEqual:@""]) { NSLog(@"[probe] commit failed: %@ %@", v.string, probeText); return false; }
        [v insertText:@"ＡＢＣ１２３！あいう😀" replacementRange:NSMakeRange(NSNotFound,0)];
        [v setMarkedText:@"取消" selectedRange:NSMakeRange(2,0) replacementRange:NSMakeRange(NSNotFound,0)];
        [v doCommandBySelector:@selector(cancelOperation:)];
        [v doCommandBySelector:@selector(insertNewline:)];
        [v doCommandBySelector:@selector(deleteBackward:)];
        [v setMarkedText:@"かな" selectedRange:NSMakeRange(2,0) replacementRange:NSMakeRange(NSNotFound,0)];
        [v unmarkText];
        [v setMarkedText:@"送らない" selectedRange:NSMakeRange(4,0) replacementRange:NSMakeRange(NSNotFound,0)];
        v.epoch = 0; [v discard];
        BOOL ok = [probeText isEqual:@[@"日本", @"ＡＢＣ１２３！あいう😀", @"かな"]]
            && [probeKeys isEqual:@[@36, @51]] && !v.hasMarkedText && !v.string.length;
        if (!ok) NSLog(@"[probe] final failed: text=%@ keys=%@ marked=%d buffer=%@", probeText, probeKeys, v.hasMarkedText, v.string);
        probeText = nil; probeKeys = nil;
        return ok;
    }
}
