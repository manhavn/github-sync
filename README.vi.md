# GitSync Daemon

Dịch vụ đồng bộ chạy ẩn dưới nền (background service) an toàn viết bằng Rust, giúp tự động tạo gương (mirror) và đồng bộ tất cả các kho chứa (cả public và private) từ tài khoản GitHub của bạn về máy tính cá nhân. Dự án đi kèm với giao diện bảng điều khiển Web UI trực quan và hiện đại.

🇬🇧 [Xem phiên bản Tiếng Anh tại đây (English version)](README.md)

---

## ✨ Tính Năng

- **Chế độ Chạy Ẩn (Background Daemon Mode)**: Hoạt động ngầm im lặng trên hệ thống (`-b` / `--background`) quản lý thông qua tệp PID và ghi nhật ký tự động vào tệp log.
- **Bảng Điều Khiển Web UI Độc Đáo**: Giao diện tối (dark-mode) với phong cách thiết kế Glassmorphism hiện đại (viết bằng HTML/CSS/JS thuần, được nhúng trực tiếp vào trong tệp nhị phân khi biên dịch).
- **Xác Thực An Toàn**: Sử dụng tính năng **Git Credential Helper** động và tạm thời thay vì lưu trực tiếp Personal Access Token (PAT) của bạn vào các tệp tin `.git/config` trên ổ đĩa.
- **Phân Loại Tự Động**: Tự động phân chia thư mục theo cấu trúc phân cấp sạch sẽ dạng: `[thư_mục_cấu_hình]/[tên_owner_repo]/[tên_repo]`.
- **Điều Khiển Qua REST API**: Các endpoint API cho phép theo dõi tiến trình, cập nhật cấu hình động, kiểm tra lỗi và ép buộc chu kỳ đồng bộ ngay lập tức.
- **Nhiều Chế Độ Đồng Bộ Thủ Công**:
  - **Force Full Sync (Đồng bộ đầy đủ)**: Kết nối tới GitHub API và đồng bộ tất cả các repo (clone repo chưa có, pull repo đã có).
  - **Sync Missing Only (Chỉ đồng bộ repo thiếu)**: Kết nối tới GitHub API nhưng chỉ clone các repo chưa tồn tại dưới local (bỏ qua repo đã có để tiết kiệm thời gian).
  - **Pull Updates Only (Chỉ kéo cập nhật - Offline API)**: Quét thư mục local và chạy `git pull` cập nhật trực tiếp, hoàn toàn bỏ qua việc gọi GitHub API để tránh bị giới hạn API rate limit.

---

## 🛠️ Yêu Cầu Hệ Thống

- **Rust & Cargo** (v1.65+)
- **Git** đã được cài đặt trên máy
- **Các công cụ biên dịch cơ bản** (`gcc`/`g++`, `pkg-config`, `openssl-dev`)

---

## 🚀 Hướng Dẫn Cài Đặt & Biên Dịch

1. Clone kho lưu trữ này về máy.
2. Biên dịch dự án ở chế độ tối ưu (release mode):
   ```bash
   cargo build --release
   ```
3. Sao chép tệp nhị phân đã biên dịch vào đường dẫn hệ thống:
   ```bash
   cp target/release/gitsync /usr/local/bin/gitsync
   ```

---

## 💻 Danh Sách Các Lệnh CLI

Lệnh `gitsync` hỗ trợ các thao tác điều khiển sau:

### 1. Cấu hình cài đặt
Thiết lập tài khoản GitHub, token truy cập PAT, đường dẫn lưu trữ cục bộ, chu kỳ đồng bộ và cổng máy chủ Web:
```bash
gitsync config --username <tên_tài_khoản> --token <token_pat> --path <đường_dẫn_local> --interval 3600 --port 9090
```

### 2. Khởi chạy dịch vụ đồng bộ
Bắt đầu chạy dịch vụ. Thêm cờ `--background` hoặc `-b` để chạy ẩn tiến trình:
```bash
gitsync start --background
```
*Sau khi chạy, bạn có thể truy cập Web UI tại địa chỉ: `http://127.0.0.1:9090`.*

### 3. Kiểm tra trạng thái
Xem dịch vụ có đang chạy ẩn hay không, cùng với các tệp tin lưu trữ và 10 dòng log mới nhất:
```bash
gitsync status
```

### 4. Đồng bộ ngay lập tức
Yêu cầu đồng bộ toàn bộ tài khoản ngay lập tức. Nếu daemon đang chạy ẩn, nó sẽ xử lý; nếu không, lệnh sẽ tự chạy đồng bộ trên terminal hiện tại:
```bash
gitsync sync
```

### 5. Dừng dịch vụ chạy ẩn
```bash
gitsync stop
```

---

## 🎨 Các API Của Web Server

Khi daemon hoạt động, nó sẽ mở các đường dẫn API sau:
- `GET /` - Trả về giao diện Web UI.
- `GET /api/status` - Lấy trạng thái hoạt động và danh sách các repo dưới định dạng JSON.
- `POST /api/config` - Lưu cấu hình mới nhận được vào bộ nhớ và ghi đè tệp cấu hình.
- `POST /api/sync` - Yêu cầu bắt đầu đồng bộ tức thì.

---

## 📂 Đường Dẫn Tệp Tin Lưu Trữ

Mọi thông tin cấu hình, PID và nhật ký hoạt động được lưu tại thư mục cấu hình của người dùng:
- **Tệp Cấu Hình JSON**: `~/.config/gitsync/config.json`
- **Tệp PID**: `~/.config/gitsync/gitsync.pid`
- **Tệp Log**: `~/.config/gitsync/gitsync.log`

Để theo dõi nhật ký hoạt động thời gian thực của daemon:
```bash
tail -f ~/.config/gitsync/gitsync.log
```

---

## 📄 Bản Quyền

Dự án này được cấp phép dưới các điều khoản của Giấy phép MIT (MIT License).
