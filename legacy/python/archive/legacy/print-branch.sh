# ฟังก์ชันช่วยแสดงกิ่งต้นไม้ 1 ชั้นพร้อมย่อหน้า
_print_branch() {
    LS_COLORS="" eza --tree --level=1 --color=always --icons=always --group-directories-first "${args[@]}" "$target" 2>/dev/null
    local target="$1" indent="$2"
    shift 2
    local args=("${@//--la/-la}")

    if command -v eza &>/dev/null; then
        eza --tree --level=1 --color=always --icons=always --group-directories-first "${args[@]}" "$target" 2>/dev/null \
            | tail -n +2 | awk -v p="$indent" '{print p $0}'
    elif command -v tree &>/dev/null; then
        tree -L 1 -C --noreport "${args[@]}" "$target" 2>/dev/null \
            | sed '1d' | awk -v p="$indent" '{print p $0}'
    else
        command ls -1 --color=always "${args[@]}" "$target" 2>/dev/null \
            | awk -v p="$indent" '{print p "├── " $0}'
    fi
}

# ฟังก์ชันแสดงผัง My Computer และแตกกิ่งต่อใต้โฟลเดอร์ปัจจุบัน
show_my_computer() {
    local trash_root="$HOME/.local/share/Trash"
    local trash_dir="$trash_root/files"
    echo -e "\n📍 ตอนนี้อยู่ที่: $PWD"
    echo "💻 My Computer"

    # 1. หมวด Home (ยกเว้นโฟลเดอร์ Trash ทั้งหมด)
    echo "├── 🏠 Home ($HOME)"
    if [[ "$PWD" == "$HOME" ]]; then
        _print_branch "$PWD" "│   " "$@"
    elif [[ "$PWD" == "$HOME/"* && "$PWD" != "$trash_root" && "$PWD" != "$trash_root/"* ]]; then
        local rel_path="${PWD#$HOME/}"
        echo "│   └── 📁 $rel_path"
        _print_branch "$PWD" "│       " "$@"
    fi

    # 2. หมวด Devices (USB / ไดรฟ์ภายนอก)
    if [ -d "/media/$USER" ] && [ "$(command ls -A "/media/$USER" 2>/dev/null)" ]; then
        echo "├── 🔌 Devices (/media/$USER)"
        if [[ "$PWD" == "/media/$USER" ]]; then
            _print_branch "$PWD" "│   " "$@"
        elif [[ "$PWD" == "/media/$USER/"* ]]; then
            local rel_dev="${PWD#/media/$USER/}"
            echo "│   └── 📁 $rel_dev"
            _print_branch "$PWD" "│       " "$@"
        fi
    fi

    # 3. หมวด File System (/)
    echo "├── 💽 File System (/)"
    if [[ "$PWD" == "/" ]]; then
        _print_branch "/" "│   " "$@"
    elif [[ "$PWD" != "$HOME" && "$PWD" != "$HOME/"* \
         && "$PWD" != "/media/$USER" && "$PWD" != "/media/$USER/"* ]]; then
        echo "│   └── 📁 ${PWD#/}"
        _print_branch "$PWD" "│       " "$@"
    fi

    # 4. หมวด Trash (รองรับทั้ง Trash/files และโฟลเดอร์แม่ Trash)
    echo -e "└── 🗑️  Trash  [$(command ls -1 "$trash_dir" 2>/dev/null | wc -l) รายการ]"
    if [[ "$PWD" == "$trash_dir" ]]; then
        _print_branch "$PWD" "    " "$@"
    elif [[ "$PWD" == "$trash_dir/"* ]]; then
        local rel_trash="${PWD#$trash_dir/}"
        echo "    └── 📁 $rel_trash"
        _print_branch "$PWD" "        " "$@"
    elif [[ "$PWD" == "$trash_root" || "$PWD" == "$trash_root/"* ]]; then
        echo "    └── 📁 ${PWD#$HOME/}"
        _print_branch "$PWD" "        " "$@"
    fi
    echo ""
}

# ครอบคำสั่ง cd แบบปลอดภัยต่อสคริปต์ที่ใช้ set -e
cd() {
    builtin cd "$@" || return
    [[ $- == *i* ]] && show_my_computer
    return 0
}

# ใช้ lsx สำหรับดูผังต้นไม้โดยไม่ทับ ls เดิม
lsx() {
    show_my_computer "$@"
}

# แสดงทันทีตอนเปิด Terminal ครั้งแรก
[[ $- == *i* ]] && show_my_computer

# ล้างหน้าจอแล้ววาดผัง My Computer กลับขึ้นมาทันที
clear() {
    command clear
    [[ $- == *i* ]] && show_my_computer
}

# ทำให้การกดปุ่มลัด Ctrl + L เรียกใช้ฟังก์ชัน clear ด้านบนด้วย
[[ $- == *i* ]] && bind -x '"\C-l": "clear"'
