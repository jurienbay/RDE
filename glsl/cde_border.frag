#version 450

layout(location = 0) in vec2 inTexCoord;
layout(location = 1) in vec2 inPixelPos;

layout(location = 0) out vec4 outColor;

layout(binding = 0) uniform sampler2D appTexture;

layout(push_constant) uniform PushConstants {
    mat4 mvp;
    vec2 element_size;
    uint is_focused;
    uint is_dragging;
    uint is_pressed;
    uint title_chars[16];
} push;

const vec4 CDE_FACE_COLOR = vec4(0.73, 0.73, 0.73, 1.0); // Classic CDE Light Grey Window Face
const vec4 CDE_HIGHLIGHT  = vec4(0.95, 0.95, 0.95, 1.0);
const vec4 CDE_SHADOW     = vec4(0.42, 0.42, 0.42, 1.0);
const vec4 CDE_ETCH_BLACK = vec4(0.00, 0.00, 0.00, 1.0);

const float BEVEL_THICKNESS = 4.0;

// Sizing metrics matching your main.rs setup configuration parameters
const float BORDER_WIDTH = 6.0;
const float TITLEBAR_HEIGHT = 22.0;

// CDE HIGH-PERFORMANCE PROCEDURAL 5x7 ASCII CHARACTER RENDERING GENERATOR
bool draw_cde_ascii_glyph(uint ascii, float x, float y) {
    if (x < 0.0 || x >= 5.0 || y < 0.0 || y >= 7.0) return false;
    int ix = int(x); int iy = int(y);

    // Letter 'F' (ASCII 70)
    if (ascii == 70u || ascii == 102u) {
        if (ix == 0) return true;
        if (iy == 0 || iy == 3) return true;
    }
    // Letter 'O' (ASCII 79)
    if (ascii == 79u || ascii == 111u) {
        if ((ix == 0 || ix == 4) && iy > 0 && iy < 6) return true;
        if ((iy == 0 || iy == 6) && ix > 0 && ix < 4) return true;
    }
    // Letter 'T' (ASCII 84)
    if (ascii == 84u || ascii == 116u) {
        if (iy == 0) return true;
        if (ix == 2) return true;
    }
    // Letter 'A' (ASCII 65)
    if (ascii == 65u || ascii == 97u) {
        if (ix == 0 || ix == 4) return true;
        if (iy == 0 || iy == 3) return true;
    }
    // Letter 'P' (ASCII 80)
    if (ascii == 80u || ascii == 112u) {
        if (ix == 0) return true;
        if ((iy == 0 || iy == 3) && ix < 4) return true;
        if (ix == 4 && iy > 0 && iy < 3) return true;
    }
    // Letter '*' (ASCII 42) - Compact 5x5 centered cross/star representation
    if (ascii == 42u) {
        if (ix == 2 || iy == 3) { if (ix >= 1 && ix <= 3 && iy >= 2 && iy <= 4) return true; }
        if (abs(ix - iy) == 0 && ix >= 1 && ix <= 3) return true;
        if (ix + iy == 5 && ix >= 1 && ix <= 3) return true;
    }
    // Standard terminal underscore cursor fallback for any trailing indices
    if (ascii == 95u) {
        if (iy == 6) return true;
    }

    return false;
}

// =========================================================================
// RETRO CDE PROCEDURAL BITMAP TEXT ENGINE:
// =========================================================================
// Evaluates a tight 16x7 pixel grid container to draw block letters.
bool draw_cde_label(uint label_idx, float x, float y) {
    // Center the text inside the button boundaries by applying an offset padding
    float text_x = x - 1.0;
    float text_y = y - 4.0;

    // Boundary guard check: return false if out of the character bounding box bounds
    if (text_x < 0.0 || text_x >= 16.0 || text_y < 0.0 || text_y >= 7.0) return false;

    int ix = int(text_x);
    int iy = int(text_y);

    // ---------------------------------------------------------------------
    // WORD 0: "ONE"
    // ---------------------------------------------------------------------
    if (label_idx == 0u) {
        // Character 'O' (Columns 0 to 4)
        if ((ix == 0 || ix == 4) && iy > 0 && iy < 6) return true;
        if ((iy == 0 || iy == 6) && ix > 0 && ix < 4) return true;

        // Character 'N' (Columns 6 to 10)
        if (ix == 6 || ix == 10) return true;
        if (ix == 7 && iy == 2) return true;
        if (ix == 8 && iy == 3) return true;
        if (ix == 9 && iy == 4) return true;

        // Character 'E' (Columns 12 to 15)
        if (ix == 12) return true;
        if (iy == 0 || iy == 3 || iy == 6) { if (ix >= 12 && ix <= 15) return true; }
    }
    // ---------------------------------------------------------------------
    // WORD 1: "TWO"
    // ---------------------------------------------------------------------
    else if (label_idx == 1u) {
        // Character 'T' (Columns 0 to 4)
        if (iy == 0 && ix >= 0 && ix <= 4) return true;
        if (ix == 2) return true;

        // Character 'W' (Columns 6 to 10)
        if (ix == 6 || ix == 10) return true;
        if (iy == 5 && (ix == 7 || ix == 9)) return true;
        if (iy == 4 && ix == 8) return true;

        // Character 'O' (Columns 12 to 15)
        if ((ix == 12 || ix == 15) && iy > 0 && iy < 6) return true;
        if ((iy == 0 || iy == 6) && ix > 12 && ix < 15) return true;
    }
    // ---------------------------------------------------------------------
    // WORD 2: "THR"
    // ---------------------------------------------------------------------
    else if (label_idx == 2u) {
        // Character 'T' (Columns 0 to 4)
        if (iy == 0 && ix >= 0 && ix <= 4) return true;
        if (ix == 2) return true;

        // Character 'H' (Columns 6 to 9)
        if (ix == 6 || ix == 9) return true;
        if (iy == 3 && ix > 6 && ix < 9) return true;

        // Character 'R' (Columns 11 to 15)
        if (ix == 11) return true;
        if (iy == 0 || iy == 3) { if (ix >= 11 && ix <= 14) return true; }
        if (ix == 14 && iy > 0 && iy < 3) return true;
        if (ix == 13 && iy == 4) return true;
        if (ix == 14 && iy == 5) return true;
        if (ix == 15 && iy == 6) return true;
    }
    // ---------------------------------------------------------------------
    // WORD 3: "FOR"
    // ---------------------------------------------------------------------
    else if (label_idx == 3u) {
        // Character 'F' (Columns 0 to 4)
        if (ix == 0) return true;
        if (iy == 0 || iy == 3) { if (ix >= 0 && ix <= 4) return true; }

        // Character 'O' (Columns 6 to 9)
        if ((ix == 6 || ix == 9) && iy > 0 && iy < 6) return true;
        if ((iy == 0 || iy == 6) && ix > 6 && ix < 9) return true;

        // Character 'R' (Columns 11 to 15)
        if (ix == 11) return true;
        if (iy == 0 || iy == 3) { if (ix >= 11 && ix <= 14) return true; }
        if (ix == 14 && iy > 0 && iy < 3) return true;
        if (ix == 13 && iy == 4) return true;
        if (ix == 14 && iy == 5) return true;
        if (ix == 15 && iy == 6) return true;
    }

    return false;
}

// =========================================================================
// RETRO CDE PROCEDURAL BITMAP ICON ENGINE:
// =========================================================================
// Evaluates a 32x32 local pixel coordinate grid to draw a 90s UNIX CRT Monitor icon.
vec4 draw_cde_terminal_icon(float x, float y) {
    // Boundary check: icon asset footprint fits entirely inside a 32x32 container
    if (x < 0.0 || x >= 32.0 || y < 0.0 || y >= 32.0) return vec4(0.0);

    int ix = int(x);
    int iy = int(y);

    // Color Palette matching classic 1996 systems
    vec4 CRT_BEZEL   = vec4(0.60, 0.60, 0.60, 1.0); // Medium grey casing
    vec4 CRT_SCREEN  = vec4(0.15, 0.15, 0.20, 1.0); // Dark terminal viewport
    vec4 RETRO_GLOW  = vec4(0.00, 0.90, 0.20, 1.0); // Classic Green Phosphor prompt line

    // 1. OUTLINE & CASING: Draw the outer box shell of a CRT monitor
    if (iy >= 4 && iy <= 22) {
        if (ix >= 2 && ix <= 29) {
            // Screen Bezel Inner Border Edging Cutout
            if (iy >= 7 && iy <= 19 && ix >= 5 && ix <= 26) {
                // 2. INNER VIEWPORT SCREEN: Paint the glass canvas area
                // Draw a simple retro prompt symbol '>' and shell underscore line inside terminal text block
                if ((iy == 10 && ix == 8) ||
                    (iy == 11 && (ix == 9 || ix == 10)) ||
                    (iy == 12 && ix == 8) ||
                    (iy == 15 && ix >= 8 && ix <= 14)) {
                    return RETRO_GLOW;
                }
                return CRT_SCREEN;
            }
            // Draw a thin chiseled shadow line on the lower bezel edge
            if (iy == 22 || ix == 29) return CDE_SHADOW;
            if (iy == 4  || ix == 2)  return CDE_HIGHLIGHT;
            return CRT_BEZEL;
        }
    }

    // 3. MONITOR STAND BASE: Connect the monitor neck mount down to the panel face
    if (iy >= 23 && iy <= 25) {
        if (ix >= 12 && ix <= 19) return CRT_BEZEL;
    }
    if (iy >= 26 && iy <= 28) {
        if (ix >= 6 && ix <= 25) {
            if (iy == 28 || ix == 25) return CDE_SHADOW;
            return CRT_BEZEL;
        }
    }

    return vec4(0.0); // Transparent fall-through
}

// Evaluates a 32x32 local pixel coordinate grid to draw a 90s UNIX Trash Can icon.
vec4 draw_cde_trash_icon(float x, float y) {
    if (x < 0.0 || x >= 32.0 || y < 0.0 || y >= 32.0) return vec4(0.0);

    int ix = int(x);
    int iy = int(y);

    vec4 CAN_BODY   = vec4(0.55, 0.55, 0.55, 1.0); // Brushed metal gray
    vec4 CAN_ACCENT = vec4(0.35, 0.35, 0.35, 1.0); // Deep grooves / shadows

    // Lid handle bar
    if (iy >= 3 && iy <= 4 && ix >= 12 && ix <= 19) return CAN_BODY;

    // Trash bin lid lip
    if (iy >= 5 && iy <= 7 && ix >= 6 && ix <= 25) {
        if (iy == 7 || ix == 25) return CDE_SHADOW;
        return CAN_BODY;
    }

    // Tapered bin body walls
    if (iy >= 8 && iy <= 27) {
        // Linear formula calculation for tapered bin edges
        int inset = (iy - 8) / 4;
        int left_bound = 8 + inset;
        int right_bound = 23 - inset;

        if (ix >= left_bound && ix <= right_bound) {
            // Vertical structural ridge slots
            if ((ix == 11 && iy % 3 != 0) ||
                (ix == 15 && iy % 3 != 0) ||
                (ix == 19 && iy % 3 != 0)) {
                return CAN_ACCENT;
            }
            if (ix == right_bound || iy == 27) return CDE_SHADOW;
            return CAN_BODY;
        }
    }

    return vec4(0.0);
}

void main() {
    vec4 texColor = texture(appTexture, inTexCoord);

    // Pass 1: Render the background gray retro frame decoration.
    if (texColor.a == 0.0) {
        float x = inPixelPos.x;
        float y = inPixelPos.y;
        float w = push.element_size.x;
        float h = push.element_size.y;

	// ----------------===========================================
	// INSERTED: AUTHENTIC CDE / MOTIF LOCK SCREEN WINDOW DESIGN
	// ----------------===========================================
        if (push.is_focused == 4u) {
            vec4 shadowColor = CDE_SHADOW;
            vec4 lightColor  = CDE_HIGHLIGHT;
            vec4 finalColor  = CDE_FACE_COLOR;
            float b = 6.0; // Thick structural frame border line thickness metric

            // 1. Render 3D Chiseled Outer Bevel Framework Borders
            if (x < b || y < b) {
                finalColor = lightColor; // Top-left chiseled light accent strips
            }
            else if (x > (w - b) || y > (h - b)) {
                finalColor = shadowColor; // Bottom-right chiseled shadow accent strips
            }

            // 2. SUNKEN DIALOG ENTRY FIELD: Generate a recessed 3D input field box for password strings
            // Positioning metrics: Centered horizontally, pushed toward the lower frame segment
            float inputW = 240.0;
            float inputH = 24.0;
            float inputX = (w - inputW) / 2.0;
            float inputY = 40.0; // Pushed near the box floor segment base limits

            if (x >= inputX && x <= (inputX + inputW) && y >= inputY && y <= (inputY + inputH)) {
                // Recess box fills with a solid flat text canvas sheet color
                finalColor = vec4(0.85, 0.85, 0.85, 1.0); // Classic light ash paper canvas background

                // Invert the edge colors to force a sunken/recessed look instead of pop-out geometry
                float eb = 2.0; // Element frame line limits boundary buffer steps
                if (x < (inputX + eb) || y < (inputY + eb)) {
                    finalColor = shadowColor; // Sunken top-left border accent lines
                }
                else if (x > (inputX + inputW) - eb || y > (inputY + inputH) - eb) {
                    finalColor = lightColor; // Sunken bottom-right border accent lines
                }

                // ----------------===========================================
                // RENDER ASTERISK ECHO STRINGS
                // ------------------------------------------------===========
                // Shift coordinates relative to the interior of the text box container margins
                float text_local_x = x - (inputX + 8.0); // 8px left margin padding offset
                float text_local_y = y - (inputY + 8.0); // 8px top margin vertically centered inside the 24px box

                if (text_local_x >= 0.0 && text_local_y >= 0.0 && text_local_y < 7.0) {
                    // Each asterisk character token takes 8 pixels of horizontal workspace (5px glyph + 3px padding buffer spacing step)
                    uint asterisk_index = uint(floor(text_local_x / 8.0));
                    float glyph_x = mod(text_local_x, 8.0);

                    // push.is_pressed holds our dynamic password_length counter!
                    if (asterisk_index < push.is_pressed) {
                        // Call your text generator passing ASCII code 42
                        if (draw_cde_ascii_glyph(42u, glyph_x, text_local_y)) {
                            finalColor = CDE_ETCH_BLACK; // Stark black font etch ink
                        }
                    }
                }
            }

            // Thin outer black etch line (Highest priority on extreme edges)
            if (x < 1.0 || y < 1.0 || x >= (w - 1.0) || y >= (h - 1.0)) {
                finalColor = CDE_ETCH_BLACK;
            }

            outColor = finalColor;
            return; // Exit main pipeline execution early to short-circuit window decorators!
        }
	////

	// Thin outer black etch line (Highest priority on extreme edges)
        if (x < 1.0 || y < 1.0 || x >= (w - 1.0) || y >= (h - 1.0)) {
            outColor = CDE_ETCH_BLACK;
	    return;
	}

	// Bevel Borders (Strictly between 1.0 and BEVEL_THICKNESS)
        if (x < BEVEL_THICKNESS || y < BEVEL_THICKNESS || x > (w - BEVEL_THICKNESS) || y > (h - BEVEL_THICKNESS)) {

	    // Calculate normalized slope coordinates to handle rectangular surfaces
            float normX = x / w;
            float normY = y / h;

	    if (normY < (1.0 - normX)) {
		// We are in the top-left half triangle of the surface
		// Left border highlight OR  Top border highlight
                outColor = (x < BEVEL_THICKNESS && x <= y) ? CDE_HIGHLIGHT : CDE_HIGHLIGHT;
            } else {
		// We are in the bottom-right half triangle of the surface
		// Right border shadow OR Bottom border shadow
                outColor = (x > (w - BEVEL_THICKNESS) && (w - x) <= (h - y)) ? CDE_SHADOW : CDE_SHADOW;
            }
            return;
	}

	float corner_size = 24.0;
	// Isolate coordinate checks relative to all four frame intersecting corners
        bool near_x = (x < corner_size || x > (w - corner_size));
        bool near_y = (y < corner_size || y > (h - corner_size));

	// If we hit the cross-section boundaries, draw a crisp, thin 1-pixel shadow seam
        if ((near_x && (abs(y - corner_size) < 0.8 || abs(y - (h - corner_size)) < 0.8)) ||
            (near_y && (abs(x - corner_size) < 0.8 || abs(x - (w - corner_size)) < 0.8))) {

            // Safety Check: Only etch if we are inside the window decorations limits (Focus 0 and 1)
            if (push.is_focused == 1u || push.is_focused == 0u) {
                outColor = CDE_SHADOW;
                return; // Line complete!
            }
        }

	// Catch our Titlebar layouts matching Step 3 configuration values
        if (push.is_focused == 1u || push.is_focused == 0u) {
		// Titlebar Top Header Area
		if (y < TITLEBAR_HEIGHT) {
			// CONSTANTS TRACKING MAIN.RS METRICS
			float btn_size = 16.0;
			float btn_offset = 6.0;

			// Horizontal Alignment: Positioned exactly near the right border edge
			// Close Button parameters
			float close_left = w - BORDER_WIDTH - btn_offset - btn_size;
			float close_right = w - BORDER_WIDTH - btn_offset;
			float btn_top = BEVEL_THICKNESS;
			float btn_bottom = btn_top + btn_size;

			// Minimize Button boundaries (26.0 pixels shifted inward from the right)
			float min_left = w - BORDER_WIDTH - 26.0 - btn_size;
			float min_right = w - BORDER_WIDTH - 26.0;

			// Positioned exactly near the left border edge
			float menu_left  = BORDER_WIDTH + btn_offset; // Resolves to 12.0
			float menu_right = menu_left + btn_size;      // Resolves to 28.0

			// CHECK IF CURSOR SATISFIES LEFT MENU BUTTON REGION
			if (x >= menu_left && x <= menu_right && y >= btn_top && y <= btn_bottom) {
				float bx = x - menu_left;
				float by = y - btn_top;
				float eth = 1.5;

				// Dynamic State Swap Selection
				vec4 current_highlight = (push.is_pressed == 3u) ? CDE_SHADOW : CDE_HIGHLIGHT;
				vec4 current_shadow    = (push.is_pressed == 3u) ? CDE_HIGHLIGHT : CDE_SHADOW;

				if (bx < eth || by < eth) {
					outColor = current_highlight; // Top-Left popping highlight
				} else if (bx > (btn_size - eth) || by > (btn_size - eth)) {
					outColor = current_shadow;    // Bottom-Right shadow edge
				} else {
					// Draw a traditional thick horizontal window dash inside the button
					if (bx >= 3.0 && bx <= (btn_size - 3.0) && by >= 7.0 && by <= 9.0) {
						outColor = CDE_ETCH_BLACK;
					} else {
						outColor = CDE_FACE_COLOR;
					}
				}
				return;
			}

			// CHECK IF CURSOR COORDINATE SAT SATISFIES CLOSE BUTTON REGION
			if (x >= close_left && x <= close_right && y >= btn_top && y <= btn_bottom) {
				float bx = x - close_left;
				float by = y - btn_top;
				float eth = 1.5; // Button bevel thickness

				vec4 current_highlight = (push.is_pressed == 1u) ? CDE_SHADOW : CDE_HIGHLIGHT;
				vec4 current_shadow    = (push.is_pressed == 1u) ? CDE_HIGHLIGHT : CDE_SHADOW;

				// Draw pop-out 3D borders around the button square
				if (bx < eth || by < eth) {
					outColor = current_highlight; // Top-Left popping highlight
				} else if (bx > (btn_size - eth) || by > (btn_size - eth)) {
					outColor = current_shadow;    // Bottom-Right shadow edge
				} else {
					// Render procedural asymmetric 'X' Cross lines in the button center
					// Padding inward 4 pixels to isolate the cross strokes nicely
					if (bx >= 4.0 && bx <= (btn_size - 4.0) && by >= 4.0 && by <= (btn_size - 4.0)) {
						// Diagonal checks mapping: main slope or inverted slope lines
						float delta1 = abs(bx - by);
						float delta2 = abs(bx - (btn_size - by));
						// Black cross strokes 'else'  Button inner body fill
						outColor = (delta1 < 1.2 || delta2 < 1.5) ? CDE_ETCH_BLACK : CDE_FACE_COLOR;
					} else {
						outColor = CDE_FACE_COLOR; // Button margin padding fill
					}
				}
				return;
			}

			// EVALUATE MINIMIZE BUTTON
			if (x >= min_left && x <= min_right && y >= btn_top && y <= btn_bottom) {
				float bx = x - min_left;
				float by = y - btn_top;
				float eth = 1.5;

				vec4 current_highlight = (push.is_pressed == 2u) ? CDE_SHADOW : CDE_HIGHLIGHT;
				vec4 current_shadow    = (push.is_pressed == 2u) ? CDE_HIGHLIGHT : CDE_SHADOW;

				if (bx < eth || by < eth) {
					outColor = current_highlight; // Raised edge highlight
				} else if (bx > (btn_size - eth) || by > (btn_size - eth)) {
					outColor = current_shadow;    // Shadow edge
				} else {
					// Draw a tiny solid retro dot icon inside the center of the square
					if (bx >= 6.0 && bx <= 10.0 && by >= 6.0 && by <= 10.0) {
						outColor = CDE_ETCH_BLACK;
					} else {
						outColor = CDE_FACE_COLOR;
					}
				}
				return;
			}

			// Add classic CDE sunken etch line separating the titlebar from client area
			// Sunken ridge separator accent tracking line
			if (y >= (TITLEBAR_HEIGHT - 2.0)) {
			    outColor = CDE_SHADOW; // Dark shadow edge
			} else if (y >= (TITLEBAR_HEIGHT - 3.0)) {
			    outColor = CDE_HIGHLIGHT; // Light highlight edge
			} else {
				// The blue focus bar spans continuously across the header between widgets.
				// The title text area cuts a hole through the decorator frame, allowing the running
				// client application's window title text string to render cleanly on top!
				float text_area_left  = (w / 2.0) - 85.0; // Widened slightly to prevent clipping longer title strings
				float text_area_right = (w / 2.0) + 85.0;

				if (x >= menu_right && x <= min_left) {
				    // Check if we land inside the absolute center title text window region
				    if (x >= text_area_left && x <= text_area_right) {
					float local_text_x = x - text_area_left;
					float local_text_y = y - 7.0; // Center text vertically within the 22px title bar

					// Each letter space takes 6 pixels of width (5px text grid + 1px spacing gutter)
					uint char_index = uint(floor(local_text_x / 6.0));
					float glyph_x = mod(local_text_x, 6.0);

					if (char_index < 16u) {
						uint target_ascii = push.title_chars[char_index];
						// If we hit a valid character stroke, print crisp text colors!
						if (target_ascii != 0u && draw_cde_ascii_glyph(target_ascii, glyph_x, local_text_y)) {
							// Focused active text is stark crisp white, unfocused text maps dark grey
							outColor = (push.is_focused == 1u) ? vec4(1.0, 1.0, 1.0, 1.0) : vec4(0.30, 0.30, 0.30, 1.0);
							return;
						}
					}
					// Underneath the letter glyphs, draw the clean solid slate focus bar color
					outColor = (push.is_focused == 1u) ? vec4(0.29, 0.43, 0.65, 1.0) : CDE_FACE_COLOR;
				    }
				    // Draw the iconic 3D ribbed handles filling all remaining empty space
				    else {
				        // repeating vertical ridge lines spaced 4px apart
				        float local_grid = mod(x, 4.0);

				        if (local_grid < 1.0 && y >= BEVEL_THICKNESS && y <= (TITLEBAR_HEIGHT - 4.0)) {
						outColor = CDE_SHADOW;
				        } else if (local_grid >= 1.0 && local_grid < 2.0 && y >= BEVEL_THICKNESS && y <= (TITLEBAR_HEIGHT - 4.0)) {
						outColor = CDE_HIGHLIGHT;
				        } else {
						// Underneath the ridges, the focus bar color bleeds across.
						// Check if the current processing layout frame holds active focus!
						// Update active slate blue highlights dynamically based on focus status!
						// Authentic 1996 Active CDE Slate Blue color tone signature OR
						// Passive Unfocused Grey frame fill color token. CDE dark dull slate blue: #004e98 or #4a6fa5
						outColor = (push.is_focused == 1u) ? vec4(0.29, 0.43, 0.65, 1.0) : CDE_FACE_COLOR;
				        }
				    }
				} else {
				    // Protect button margins and padding gutters
				    outColor = CDE_FACE_COLOR;
				}
				// XXXX
			}
			return;
		}

		// Side border strips layout
	        if (x < BORDER_WIDTH || x > (w - BORDER_WIDTH)) {
	            outColor = CDE_FACE_COLOR;
		    return;
		}

	        // The Center Client Window Viewport Space Area
	        // Cut a hole through the frame!
	        // This forces Vulkan to skip writing any background colors here,
	        // allowing your transparent app pixels to blend directly over the desktop teal!
	        discard;
	}

        // NEW CUSTOM FRONT PANEL DOCK LOGIC PASS:
        if ((push.is_focused & 0xFu) == 2u) {
		// Extract the encoded active workspace index from the upper bits!
            uint active_desk_idx = push.is_focused >> 4u;

            // Localized bounding boxes representing the Workspace Switcher in the exact center
            float switch_left = (w / 2.0) - 50.0;
            float switch_right = (w / 2.0) + 50.0;
            float switch_top = 8.0;
            float switch_bottom = h - 8.0;

            // Draw the sunken interior border for the center Workspace Switcher box
            if (x >= switch_left && x <= switch_right && y >= switch_top && y <= switch_bottom) {
                float bx = x - switch_left;
                float by = y - switch_top;
                float bw = switch_right - switch_left;
                float bh = switch_bottom - switch_top;
                float eth = 2.0;

                // INVERTED BEVELS: Top/Left get Shadow, Bottom/Right get Highlight to sink the box inward
                if (bx < eth || by < eth) {
                    outColor = CDE_SHADOW;
                } else if (bx > (bw - eth) || by > (bh - eth)) {
                    outColor = CDE_HIGHLIGHT;
		} else {
			// Main flat gray panel desktop housing body color
			// STACKED 2x2 WORKSPACE SWITCHER MATRIX.
			float inner_bw = bw - (eth * 2.0);
			float inner_bh = bh - (eth * 2.0);

			// Divide the switcher canvas precisely down the center
			float col_w = inner_bw / 2.0;
			float row_h = inner_bh / 2.0;

			float local_x = bx - eth;
			float local_y = by - eth;

			// Calculate the grid cell coordinates [0 or 1]
			uint col_idx = uint(floor(local_x / col_w));
			uint row_idx = uint(floor(local_y / row_h));

			// Clamp indexes to shield against margin bleeding errors
			if (col_idx > 1u) col_idx = 1u;
			if (row_idx > 1u) row_idx = 1u;

			// Map grid position directly to your 0-3 room index tokens
			// Row 0: 0 (ONE), 1 (TWO) | Row 1: 2 (THREE), 3 (FOUR)
			uint cell_token = (row_idx * 2u) + col_idx;

			// Isolate the processing point relative to the active cell boundaries
			float cell_x = local_x - (float(col_idx) * col_w);
			float cell_y = local_y - (float(row_idx) * row_h);

			float b_bevel = 1.5;

			// Render raised bevel paths inside each discrete quad block
			if (cell_x < b_bevel || cell_y < b_bevel) {
			    outColor = CDE_HIGHLIGHT;
			} else if (cell_x > (col_w - b_bevel) || cell_y > (row_h - b_bevel)) {
			    outColor = CDE_SHADOW;
			} else {
			    // Pass coordinate scales inward to frame your text labels safely
			    if (draw_cde_label(cell_token, cell_x - (col_w - 16.0)/2.0, cell_y - (row_h - 7.0)/2.0)) {
			        outColor = vec4(0.20, 0.20, 0.20, 1.0); // Sunken slate font etch
			    } else {
			        if (cell_token == active_desk_idx) {
			            outColor = vec4(0.29, 0.43, 0.65, 1.0); // Active blue indicator
			        } else {
			            outColor = CDE_FACE_COLOR;
			        }
			    }
			}
                }
            } else {
		// Draw a classic 3D sunken vertical strip separating the panel modules
                if (abs(x - (switch_left - 10.0)) < 1.0 || abs(x - (switch_right + 10.0)) < 1.0) {
                    if (y >= 4.0 && y <= h - 4.0) {
                        // Left line edge gets dark shadow, right line edge gets highlight
                        outColor = (x == floor(switch_left - 10.0) || x == floor(switch_right + 10.0)) ? CDE_SHADOW : CDE_HIGHLIGHT;
                        return;
                    }
                }

		// Define an action button housing on the Left Wing (Terminal Launch button location)
		// Left Wing: Terminal Launcher bounds
                float left_btn_x1 = 20.0;
                float left_btn_x2 = 64.0;

                // Right Wing: Trash Can bounds (Mirrored to the opposite side of a 600px panel)
                float right_btn_x1 = w - 64.0; // 536.0
                float right_btn_x2 = w - 20.0; // 580.0

		float btn_top    = 10.0;
                float btn_bottom = 54.0;

		// CHECK LEFT BUTTON
                if (x >= left_btn_x1 && x <= left_btn_x2 && y >= btn_top && y <= btn_bottom) {
                    float bx = x - left_btn_x1;
                    float by = y - btn_top;
                    float bw = left_btn_x2 - left_btn_x1;
                    float bh = btn_bottom - btn_top;
                    float eth = 2.0;

                    if (bx < eth || by < eth) { outColor = CDE_SHADOW; }
                    else if (bx > (bw - eth) || by > (bh - eth)) { outColor = CDE_HIGHLIGHT; }
                    else {
                        float icon_x = bx - 5.0; float icon_y = by - 5.0;
                        vec4 icon_pixel = draw_cde_terminal_icon(icon_x, icon_y);
                        outColor = (icon_pixel.a > 0.0) ? icon_pixel : CDE_FACE_COLOR;
                    }
                } else if (x >= right_btn_x1 && x <= right_btn_x2 && y >= btn_top && y <= btn_bottom) {
		// NEW SEGMENT: CHECK RIGHT BUTTON
                    float bx = x - right_btn_x1;
                    float by = y - btn_top;
                    float bw = right_btn_x2 - right_btn_x1;
                    float bh = btn_bottom - btn_top;
                    float eth = 2.0;

                    // Sunken bevel frame matching the original layout style
                    if (bx < eth || by < eth) { outColor = CDE_SHADOW; } else if (bx > (bw - eth) || by > (bh - eth)) { outColor = CDE_HIGHLIGHT; } else {
                        float icon_x = bx - 5.0; float icon_y = by - 5.0;
                        vec4 icon_pixel = draw_cde_trash_icon(icon_x, icon_y);
                        outColor = (icon_pixel.a > 0.0) ? icon_pixel : CDE_FACE_COLOR;
                    }
                } else {
			// Default flat gray body fallback housing for the rest of the dock structure
			outColor = CDE_FACE_COLOR;
		}
            }
	    return;
        }

    // Pass 2: Client App Surface Area.
    } else {
        // WIREFRAME INTERACTION CONTROLLER INTERCEPTOR.
        // If the window is actively being dragged across the teal workspace desktop,
        // we cut out its interior application pixels completely, leaving a clean hollow space!
        if (push.is_dragging == 1u) {
            discard;
        }

	// If the texture is completely transparent/empty, render bright Magenta
	// to prove the shader is executing this branch, but the texture is empty!
        if (texColor.a < 0.05) {
            outColor = vec4(1.0, 0.0, 1.0, 1.0);  // Bright Retro Pink/Magenta
        } else {
            // Premultiply RGB colors by the texture's native alpha channel value.
            // This unblocks Vulkan's color blend attachments, allowing background elements
            // to bleed through for semi-transparency.
            outColor = vec4(texColor.rgb * texColor.a, texColor.a);
        }
    }
}
