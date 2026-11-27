# เก็บข้อมูลแต่ละบรรทัดของผังต้นไม้และตำแหน่งจริงคู่กัน (รองรับพรีวิวโฟลเดอร์ที่ชี้อยู่)
_build_tree_data() {
    _TREE_LINES=()
    _TREE_PATHS=()
    local show_hidden="$1"
    local preview_target="$2"
    local trash_root="$HOME/.local/share/Trash"
    local trash_dir="$trash_root/files"

    _add_node() {
        _TREE_LINES+=("$1")
        _TREE_PATHS+=("$2")
    }

    _append_children() {
        local target="$1" indent="$2" allow_select="$3" preview_path="$4"
        local raw_names=() disp_names=()
        local eza_opts=(--1 --icons=always --color=always --group-directories-first)
        local plain_opts=(--1 --icons=never --color=never --group-directories-first)

        if [[ "$show_hidden" == "1" ]]; then
            eza_opts+=(-a)
            plain_opts+=(-a)
        fi

        if command -v eza &>/dev/null; then
            mapfile -t raw_names < <(LS_COLORS="" eza "${plain_opts[@]}" "$target" 2>/dev/null)
            mapfile -t disp_names < <(LS_COLORS="" eza "${eza_opts[@]}" "$target" 2>/dev/null)
        else
            local ls_opts=(-1 --group-directories-first)
            [[ "$show_hidden" == "1" ]] && ls_opts+=(-A)
            mapfile -t raw_names < <(command ls "${ls_opts[@]}" "$target" 2>/dev/null)
            disp_names=("${raw_names[@]}")
        fi

        local total_items=${#raw_names[@]}
        local limit=$total_items
        local truncated=0

        # จำกัดพรีวิวไม่เกิน 15 รายการ เพื่อป้องกันหน้าจอล้นเวลาชี้โฟลเดอร์ขนาดใหญ่
        if [[ "$allow_select" == "0" && $total_items -gt 15 ]]; then
            limit=15
            truncated=$((total_items - 15))
        fi

        local i branch sub_indent full_path sel_val
        for ((i=0; i<limit; i++)); do
            if (( i == limit - 1 && truncated == 0 )); then
                branch="└── "
                sub_indent="${indent}    "
            else
                branch="├── "
                sub_indent="${indent}│   "
            fi

            full_path="${target%/}/${raw_names[$i]}"
            if [[ "$allow_select" == "1" ]]; then
                sel_val="$full_path"
            else
                sel_val=""
            fi

            _add_node "${indent}${branch}${disp_names[$i]}" "$sel_val"

            # กางผังย่อย 1 ชั้นอัตโนมัติ เฉพาะเมื่อเป็นโฟลเดอร์ที่ตัวชี้ <= กำลังชี้อยู่
            if [[ -n "$preview_path" && "$full_path" == "$preview_path" && -d "$full_path" ]]; then
                _append_children "$full_path" "$sub_indent" "0" ""
            fi
        done

        if (( truncated > 0 )); then
            _add_node "${indent}└── ... (อีก $truncated รายการ)" ""
        fi
    }

    _add_node "📍 ตอนนี้อยู่ที่: $PWD" ""
    _add_node "💻 My Computer" ""

    # 1. หมวด Home
    _add_node "├── 🏠 Home ($HOME)" "$HOME"
    if [[ "$PWD" == "$HOME" ]]; then
        _append_children "$PWD" "│   " "1" "$preview_target"
    elif [[ "$PWD" == "$HOME/"* && "$PWD" != "$trash_root" && "$PWD" != "$trash_root/"* ]]; then
        _add_node "│   └── 📁 ${PWD#$HOME/}" "$PWD"
        _append_children "$PWD" "│       " "1" "$preview_target"
    elif [[ "$preview_target" == "$HOME" ]]; then
        _append_children "$HOME" "│   " "0" ""
    fi

    # 2. หมวด Devices (USB / ไดรฟ์ภายนอก)
    if [ -d "/media/$USER" ] && [ "$(command ls -A "/media/$USER" 2>/dev/null)" ]; then
        _add_node "├── 🔌 Devices (/media/$USER)" "/media/$USER"
        if [[ "$PWD" == "/media/$USER" ]]; then
            _append_children "$PWD" "│   " "1" "$preview_target"
        elif [[ "$PWD" == "/media/$USER/"* ]]; then
            _add_node "│   └── 📁 ${PWD#/media/$USER/}" "$PWD"
            _append_children "$PWD" "│       " "1" "$preview_target"
        elif [[ "$preview_target" == "/media/$USER" ]]; then
            _append_children "/media/$USER" "│   " "0" ""
        fi
    fi

    # 3. หมวด File System (/)
    _add_node "├── 💽 File System (/)" "/"
    if [[ "$PWD" == "/" ]]; then
        _append_children "/" "│   " "1" "$preview_target"
    elif [[ "$PWD" != "$HOME" && "$PWD" != "$HOME/"* \
         && "$PWD" != "/media/$USER" && "$PWD" != "/media/$USER/"* ]]; then
        _add_node "│   └── 📁 ${PWD#/}" "$PWD"
        _append_children "$PWD" "│       " "1" "$preview_target"
    elif [[ "$preview_target" == "/" ]]; then
        _append_children "/" "│   " "0" ""
    fi

    # 4. หมวด Trash
    local trash_count
    trash_count=$(command ls -1 "$trash_dir" 2>/dev/null | wc -l)
    _add_node "└── 🗑️  Trash  [$trash_count รายการ]" "$trash_dir"
    if [[ "$PWD" == "$trash_dir" ]]; then
        _append_children "$PWD" "    " "1" "$preview_target"
    elif [[ "$PWD" == "$trash_dir/"* ]]; then
        _add_node "    └── 📁 ${PWD#$trash_dir/}" "$PWD"
        _append_children "$PWD" "        " "1" "$preview_target"
    elif [[ "$PWD" == "$trash_root" || "$PWD" == "$trash_root/"* ]]; then
        _add_node "    └── 📁 ${PWD#$HOME/}" "$PWD"
        _append_children "$PWD" "        " "1" "$preview_target"
    elif [[ "$preview_target" == "$trash_dir" ]]; then
        _append_children "$trash_dir" "    " "0" ""
    fi
}

# แสดงผังแบบปกติ (ไม่มีตัวชี้)
show_my_computer() {
    local show_hidden=0
    [[ "$*" == *"-a"* || "$*" == *"--all"* ]] && show_hidden=1
    _build_tree_data "$show_hidden" ""
    echo ""
    printf '%s\n' "${_TREE_LINES[@]}"
    echo ""
}

# โหมดเลื่อนตัวชี้ <= ด้วยปุ่มลูกศร พร้อมเปิด/ปิดผังโฟลเดอร์อัตโนมัติ
_tree_nav() {
    local show_hidden=0
    local cur_path=""
    local total=0 sel=-1

    _sync_tree() {
        local preview="" i
        if [[ -n "$cur_path" && -d "$cur_path" && "$cur_path" != "$PWD" ]]; then
            preview="$cur_path"
        fi
        _build_tree_data "$show_hidden" "$preview"
        total=${#_TREE_LINES[@]}
        sel=-1

        # ค้นหาตำแหน่ง index ของ cur_path หลังสร้างผังใหม่
        if [[ -n "$cur_path" ]]; then
            for ((i=0; i<total; i++)); do
                if [[ "${_TREE_PATHS[$i]}" == "$cur_path" ]]; then
                    sel=$i
                    break
                fi
            done
        fi

        # หากเพิ่งเข้าโฟลเดอร์ใหม่ ให้เลือกที่รายการแรกใต้โฟลเดอร์ปัจจุบัน
        if (( sel == -1 )); then
            for ((i=0; i<total; i++)); do
                if [[ "${_TREE_PATHS[$i]}" == "${PWD%/}/"* ]]; then
                    sel=$i
                    cur_path="${_TREE_PATHS[$i]}"
                    # สร้างผังอีกรอบเพื่อกางพรีวิวให้รายการแรกทันที (หากเป็นโฟลเดอร์)
                    if [[ -d "$cur_path" ]]; then
                        _build_tree_data "$show_hidden" "$cur_path"
                        total=${#_TREE_LINES[@]}
                        for ((i=0; i<total; i++)); do
                            [[ "${_TREE_PATHS[$i]}" == "$cur_path" ]] && { sel=$i; break; }
                        done
                    fi
                    break
                fi
            done
        fi

        # กรณีโฟลเดอร์ว่างเปล่า ให้ชี้อยู่ที่หมวดหลักแรก
        if (( sel == -1 )); then
            for ((i=0; i<total; i++)); do
                if [[ -n "${_TREE_PATHS[$i]}" ]]; then
                    sel=$i
                    cur_path="${_TREE_PATHS[$i]}"
                    break
                fi
            done
        fi
    }

    _render_nav() {
        local idx
        for ((idx=0; idx<total; idx++)); do
            if (( idx == sel )); then
                printf '%s \e[1;33m<=\e[0m\n' "${_TREE_LINES[$idx]}"
            else
                printf '%s\n' "${_TREE_LINES[$idx]}"
            fi
        done
    }

    _sync_tree
    trap 'printf "\e[?25h"; trap - INT; return' INT
    printf '\e[?25l\n'
    _render_nav

    local key rest target_path old_pwd prev_total
    while true; do
        IFS= read -rsn1 key
        if [[ "$key" == $'\e' ]]; then
            read -rsn2 -t 0.05 rest
            key+="$rest"
        fi

        case "$key" in
            $'\e[B'|$'\eOB') # ลูกศรลง
                prev_total=$total
                while true; do
                    sel=$(( (sel + 1) % total ))
                    [[ -n "${_TREE_PATHS[$sel]}" ]] && break
                done
                cur_path="${_TREE_PATHS[$sel]}"
                printf '\e[%dA\r\e[J' "$prev_total"
                _sync_tree
                _render_nav
                ;;
            $'\e[A'|$'\eOA') # ลูกศรขึ้น
                prev_total=$total
                while true; do
                    sel=$(( (sel - 1 + total) % total ))
                    [[ -n "${_TREE_PATHS[$sel]}" ]] && break
                done
                cur_path="${_TREE_PATHS[$sel]}"
                printf '\e[%dA\r\e[J' "$prev_total"
                _sync_tree
                _render_nav
                ;;
            $'\e[C'|$'\eOC') # ลูกศรขวา (เจาะเข้าโฟลเดอร์ที่ชี้อยู่)
                target_path="${_TREE_PATHS[$sel]}"
                if [[ -d "$target_path" && "$target_path" != "$PWD" ]]; then
                    prev_total=$total
                    printf '\e[%dA\r\e[J' "$prev_total"
                    builtin cd "$target_path" 2>/dev/null
                    cur_path=""
                    _sync_tree
                    _render_nav
                fi
                ;;
            $'\e[D'|$'\eOD') # ลูกศรซ้าย (ถอยกลับ 1 ชั้น)
                if [[ "$PWD" != "/" ]]; then
                    prev_total=$total
                    old_pwd="$PWD"
                    printf '\e[%dA\r\e[J' "$prev_total"
                    builtin cd .. 2>/dev/null
                    cur_path="$old_pwd"
                    _sync_tree
                    _render_nav
                fi
                ;;
            "") # ปุ่ม Enter
                target_path="${_TREE_PATHS[$sel]}"
                printf '\e[%dA\r\e[J\e[?25h' "$total"
                trap - INT
                if [[ -d "$target_path" ]]; then
                    cd "$target_path"
                elif [[ -f "$target_path" ]]; then
                    _build_tree_data "$show_hidden" ""
                    printf '%s\n\n' "${_TREE_LINES[@]}"
                    xdg-open "$target_path" &>/dev/null &
                    echo "🚀 เปิดไฟล์: $target_path"
                fi
                break
                ;;
            $'\e'|q|Q) # ปุ่ม Esc หรือ q (ยกเลิก)
                printf '\e[%dA\r\e[J\e[?25h' "$total"
                trap - INT
                _build_tree_data "$show_hidden" ""
                printf '%s\n\n' "${_TREE_LINES[@]}"
                break
                ;;
        esac
    done
}

# ครอบคำสั่ง cd, clear, lsx และผูกคีย์ลัด
cd() {
    builtin cd "$@" || return
    [[ $- == *i* ]] && show_my_computer
    return 0
}

lsx() {
    show_my_computer "$@"
}

clear() {
    command clear
    [[ $- == *i* ]] && show_my_computer
}

if [[ $- == *i* ]]; then
    bind -x '"\C-l": "clear"'
    bind -x '"\e[1;5B": "_tree_nav"'
    show_my_computer
fi