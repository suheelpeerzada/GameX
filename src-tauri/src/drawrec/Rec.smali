.class public final Lgamex/Rec;
.super Ljava/lang/Object;
.source "Rec.java"

# GameX's draw recorder. The game's draw calls are pointed here by GameX when
# it builds a recording copy; each one draws as before, then, while GameX has
# `armed` set, is written to a ring of records GameX reads over the debugger.
#
# ints:   8192 records x 6  [type, frame, canvas, bitmap, width, height]
# floats: 8192 records x 16 [canvas matrix sx kx tx ky sy ty, src l t r b, args...]
# types:  1 draw at x,y   2 draw src rect -> dst rect   3 draw with a matrix
#         8 frame begins (onDraw)   9 frame posted (unlockCanvasAndPost)
# targets: pairs [canvas, bitmap] for canvases drawing into a bitmap.

.field public static armed:I
.field public static frame:I
.field public static n:I
.field public static ints:[I
.field public static floats:[F
.field public static targets:[I
.field public static tn:I
.field public static want:I
.field public static got:I
.field public static pw:I
.field public static ph:I
.field public static pixels:[I
.field static m:Landroid/graphics/Matrix;
.field static mv:[F

.method public constructor <init>()V
    .registers 1
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    return-void
.end method

# Takes the next record and fills its header and the canvas's matrix.
# Returns where its floats start, or -1 when GameX isn't recording.
.method static declared-synchronized slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    .registers 10

    sget v0, Lgamex/Rec;->armed:I
    if-nez v0, :on
    const/4 v0, -0x1
    return v0

    :on
    sget-object v0, Lgamex/Rec;->ints:[I
    if-nez v0, :ready
    const v0, 0xc000
    new-array v0, v0, [I
    sput-object v0, Lgamex/Rec;->ints:[I
    const v1, 0x20000
    new-array v1, v1, [F
    sput-object v1, Lgamex/Rec;->floats:[F
    new-instance v1, Landroid/graphics/Matrix;
    invoke-direct {v1}, Landroid/graphics/Matrix;-><init>()V
    sput-object v1, Lgamex/Rec;->m:Landroid/graphics/Matrix;
    const/16 v1, 0x9
    new-array v1, v1, [F
    sput-object v1, Lgamex/Rec;->mv:[F

    :ready
    sget v1, Lgamex/Rec;->n:I
    add-int/lit8 v2, v1, 0x1
    sput v2, Lgamex/Rec;->n:I
    and-int/lit16 v1, v1, 0x1fff
    mul-int/lit8 v2, v1, 0x6
    aput p0, v0, v2
    add-int/lit8 v3, v2, 0x1
    sget v4, Lgamex/Rec;->frame:I
    aput v4, v0, v3
    add-int/lit8 v3, v2, 0x2
    invoke-static {p1}, Ljava/lang/System;->identityHashCode(Ljava/lang/Object;)I
    move-result v4
    aput v4, v0, v3
    add-int/lit8 v3, v2, 0x3
    invoke-static {p2}, Ljava/lang/System;->identityHashCode(Ljava/lang/Object;)I
    move-result v4
    aput v4, v0, v3
    if-eqz p2, :matrix
    sget v3, Lgamex/Rec;->want:I
    if-ne v3, v4, :size
    invoke-static {p2}, Lgamex/Rec;->grab(Landroid/graphics/Bitmap;)V
    :size
    add-int/lit8 v3, v2, 0x4
    invoke-virtual {p2}, Landroid/graphics/Bitmap;->getWidth()I
    move-result v4
    aput v4, v0, v3
    add-int/lit8 v3, v2, 0x5
    invoke-virtual {p2}, Landroid/graphics/Bitmap;->getHeight()I
    move-result v4
    aput v4, v0, v3

    :matrix
    shl-int/lit8 v2, v1, 0x4
    if-eqz p1, :done
    sget-object v5, Lgamex/Rec;->floats:[F
    sget-object v3, Lgamex/Rec;->m:Landroid/graphics/Matrix;
    sget-object v4, Lgamex/Rec;->mv:[F
    :try_start_0
    invoke-virtual {p1, v3}, Landroid/graphics/Canvas;->getMatrix(Landroid/graphics/Matrix;)V
    invoke-virtual {v3, v4}, Landroid/graphics/Matrix;->getValues([F)V
    :try_end_0
    .catchall {:try_start_0 .. :try_end_0} :done
    const/4 v3, 0x0
    const/4 v6, 0x6
    :copy
    if-ge v3, v6, :done
    aget v0, v4, v3
    add-int v1, v2, v3
    aput v0, v5, v1
    add-int/lit8 v3, v3, 0x1
    goto :copy

    :done
    return v2
.end method

# Copies a bitmap's pixels for GameX (it asked for this one by setting `want`).
.method static grab(Landroid/graphics/Bitmap;)V
    .registers 12

    invoke-virtual {p0}, Landroid/graphics/Bitmap;->getWidth()I
    move-result v0
    invoke-virtual {p0}, Landroid/graphics/Bitmap;->getHeight()I
    move-result v1
    mul-int v2, v0, v1
    if-lez v2, :skip
    const v4, 0x400000
    if-gt v2, v4, :skip
    move-object v3, p0
    new-array v4, v2, [I
    const/4 v5, 0x0
    move v6, v0
    const/4 v7, 0x0
    const/4 v8, 0x0
    move v9, v0
    move v10, v1
    :try_start_0
    invoke-virtual/range {v3 .. v10}, Landroid/graphics/Bitmap;->getPixels([IIIIIII)V
    :try_end_0
    .catchall {:try_start_0 .. :try_end_0} :skip
    sput-object v4, Lgamex/Rec;->pixels:[I
    sput v0, Lgamex/Rec;->pw:I
    sput v1, Lgamex/Rec;->ph:I
    invoke-static {p0}, Ljava/lang/System;->identityHashCode(Ljava/lang/Object;)I
    move-result v2
    sput v2, Lgamex/Rec;->got:I

    :skip
    const/4 v2, 0x0
    sput v2, Lgamex/Rec;->want:I
    return-void
.end method

# Writes a Rect (or -1 for none) at floats[at..at+4].
.method static rect([FILandroid/graphics/Rect;)V
    .registers 6

    if-nez p2, :has
    const/high16 v0, -0x40800000
    aput v0, p0, p1
    return-void

    :has
    iget v0, p2, Landroid/graphics/Rect;->left:I
    int-to-float v0, v0
    aput v0, p0, p1
    add-int/lit8 v1, p1, 0x1
    iget v0, p2, Landroid/graphics/Rect;->top:I
    int-to-float v0, v0
    aput v0, p0, v1
    add-int/lit8 v1, p1, 0x2
    iget v0, p2, Landroid/graphics/Rect;->right:I
    int-to-float v0, v0
    aput v0, p0, v1
    add-int/lit8 v1, p1, 0x3
    iget v0, p2, Landroid/graphics/Rect;->bottom:I
    int-to-float v0, v0
    aput v0, p0, v1
    return-void
.end method

.method static rectF([FILandroid/graphics/RectF;)V
    .registers 6

    if-nez p2, :has
    const/high16 v0, -0x40800000
    aput v0, p0, p1
    return-void

    :has
    iget v0, p2, Landroid/graphics/RectF;->left:F
    aput v0, p0, p1
    add-int/lit8 v1, p1, 0x1
    iget v0, p2, Landroid/graphics/RectF;->top:F
    aput v0, p0, v1
    add-int/lit8 v1, p1, 0x2
    iget v0, p2, Landroid/graphics/RectF;->right:F
    aput v0, p0, v1
    add-int/lit8 v1, p1, 0x3
    iget v0, p2, Landroid/graphics/RectF;->bottom:F
    aput v0, p0, v1
    return-void
.end method

.method public static drawBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V
    .registers 8

    invoke-virtual {p0, p1, p2, p3, p4}, Landroid/graphics/Canvas;->drawBitmap(Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V
    const/4 v0, 0x1
    invoke-static {v0, p0, p1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    move-result v0
    if-ltz v0, :out
    sget-object v1, Lgamex/Rec;->floats:[F
    add-int/lit8 v2, v0, 0xa
    aput p2, v1, v2
    add-int/lit8 v2, v0, 0xb
    aput p3, v1, v2
    :out
    return-void
.end method

.method public static drawBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/Rect;Landroid/graphics/Paint;)V
    .registers 9

    invoke-virtual {p0, p1, p2, p3, p4}, Landroid/graphics/Canvas;->drawBitmap(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/Rect;Landroid/graphics/Paint;)V
    const/4 v0, 0x2
    invoke-static {v0, p0, p1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    move-result v0
    if-ltz v0, :out
    sget-object v1, Lgamex/Rec;->floats:[F
    add-int/lit8 v2, v0, 0x6
    invoke-static {v1, v2, p2}, Lgamex/Rec;->rect([FILandroid/graphics/Rect;)V
    add-int/lit8 v2, v0, 0xa
    invoke-static {v1, v2, p3}, Lgamex/Rec;->rect([FILandroid/graphics/Rect;)V
    :out
    return-void
.end method

.method public static drawBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V
    .registers 9

    invoke-virtual {p0, p1, p2, p3, p4}, Landroid/graphics/Canvas;->drawBitmap(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V
    const/4 v0, 0x2
    invoke-static {v0, p0, p1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    move-result v0
    if-ltz v0, :out
    sget-object v1, Lgamex/Rec;->floats:[F
    add-int/lit8 v2, v0, 0x6
    invoke-static {v1, v2, p2}, Lgamex/Rec;->rect([FILandroid/graphics/Rect;)V
    add-int/lit8 v2, v0, 0xa
    invoke-static {v1, v2, p3}, Lgamex/Rec;->rectF([FILandroid/graphics/RectF;)V
    :out
    return-void
.end method

.method public static drawBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;Landroid/graphics/Matrix;Landroid/graphics/Paint;)V
    .registers 11

    invoke-virtual {p0, p1, p2, p3}, Landroid/graphics/Canvas;->drawBitmap(Landroid/graphics/Bitmap;Landroid/graphics/Matrix;Landroid/graphics/Paint;)V
    const/4 v0, 0x3
    invoke-static {v0, p0, p1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    move-result v0
    if-ltz v0, :out
    if-eqz p2, :out
    sget-object v1, Lgamex/Rec;->mv:[F
    invoke-virtual {p2, v1}, Landroid/graphics/Matrix;->getValues([F)V
    sget-object v2, Lgamex/Rec;->floats:[F
    add-int/lit8 v0, v0, 0xa
    const/4 v3, 0x0
    const/4 v4, 0x6
    :copy
    if-ge v3, v4, :out
    aget v5, v1, v3
    add-int v6, v0, v3
    aput v5, v2, v6
    add-int/lit8 v3, v3, 0x1
    goto :copy
    :out
    return-void
.end method

# A canvas that draws into a bitmap: remembered so GameX can tell layers
# (drawn into, then drawn onto the screen) from sprites.
.method public static declared-synchronized target(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;)V
    .registers 5

    sget-object v0, Lgamex/Rec;->targets:[I
    if-nez v0, :ready
    const/16 v0, 0x800
    new-array v0, v0, [I
    sput-object v0, Lgamex/Rec;->targets:[I
    :ready
    sget v1, Lgamex/Rec;->tn:I
    add-int/lit8 v2, v1, 0x1
    sput v2, Lgamex/Rec;->tn:I
    and-int/lit16 v1, v1, 0x3ff
    shl-int/lit8 v1, v1, 0x1
    invoke-static {p0}, Ljava/lang/System;->identityHashCode(Ljava/lang/Object;)I
    move-result v2
    aput v2, v0, v1
    add-int/lit8 v1, v1, 0x1
    invoke-static {p1}, Ljava/lang/System;->identityHashCode(Ljava/lang/Object;)I
    move-result v2
    aput v2, v0, v1
    return-void
.end method

.method public static setBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;)V
    .registers 2

    invoke-virtual {p0, p1}, Landroid/graphics/Canvas;->setBitmap(Landroid/graphics/Bitmap;)V
    invoke-static {p0, p1}, Lgamex/Rec;->target(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;)V
    return-void
.end method

.method public static post(Landroid/view/SurfaceHolder;Landroid/graphics/Canvas;)V
    .registers 4

    const/16 v0, 0x9
    const/4 v1, 0x0
    invoke-static {v0, p1, v1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    sget v0, Lgamex/Rec;->frame:I
    add-int/lit8 v0, v0, 0x1
    sput v0, Lgamex/Rec;->frame:I
    invoke-interface {p0, p1}, Landroid/view/SurfaceHolder;->unlockCanvasAndPost(Landroid/graphics/Canvas;)V
    return-void
.end method

.method public static post(Landroid/view/Surface;Landroid/graphics/Canvas;)V
    .registers 4

    const/16 v0, 0x9
    const/4 v1, 0x0
    invoke-static {v0, p1, v1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    sget v0, Lgamex/Rec;->frame:I
    add-int/lit8 v0, v0, 0x1
    sput v0, Lgamex/Rec;->frame:I
    invoke-virtual {p0, p1}, Landroid/view/Surface;->unlockCanvasAndPost(Landroid/graphics/Canvas;)V
    return-void
.end method

# Called first thing in a view's onDraw(Canvas): a new frame starts.
.method public static begin(Landroid/graphics/Canvas;)V
    .registers 3

    sget v0, Lgamex/Rec;->frame:I
    add-int/lit8 v0, v0, 0x1
    sput v0, Lgamex/Rec;->frame:I
    const/16 v0, 0x8
    const/4 v1, 0x0
    invoke-static {v0, p0, v1}, Lgamex/Rec;->slot(ILandroid/graphics/Canvas;Landroid/graphics/Bitmap;)I
    return-void
.end method
