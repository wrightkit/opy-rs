globalvar marker

rule "texture and entity compatibility":
    @Event global
    marker = Texture.MOUSE_CURSOR
    marker = f"\&black_square; {getMatchTime()}"
