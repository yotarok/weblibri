// -*- coding: utf-8 -*-

function pollConversion() {
    // Poll conversion process while modal window is open
    var bookid = $("#convertModal").data("waiting");
    var nextPoll = $("#convertModal").data("next-poll") * 1.5;
    console.log("Polling conversion status nextPoll = " + nextPoll);

    $.ajax({
        dataType: "json",
        url: API_ROOT + "/" + bookid + "/reader_status.js?enqueue=0",
        success: function(stat) {
            if (stat.is_ready) {
                window.location.href = APP_PREFIX + "/reader/" + bookid;
            } else {
                $("#convertModal").data("next-poll", nextPoll);
                setTimeout(pollConversion, nextPoll);
            }
        }
    })
}


function openReader(bookid) {
    var initDelay = 1000;
    $.ajax({
        dataType: "json",
        url: API_ROOT + "/" + bookid + "/reader_status.js",
        success: function(stat) {
            if (stat.is_ready) {
                window.location.href = APP_PREFIX + "/reader/" + bookid;
            } else {
                $("#convertModal").data("waiting", bookid)
                $("#convertModal").data("next-poll", initDelay)
                $("#convertModal").on('hide.bs.modal', function (e) {
                    clearTimeout($("#convertModal").data("timeout-handle"));
                });
                $("#convertModal").modal();
                var handle = setTimeout(pollConversion, initDelay);
                $("#convertModal").data("timeout-handle", handle)
            }
        }
    });
}

function genBookItemTableRow(data) {
    var datalinks = "";
    for (var i = 0; i < data.available_data.length; ++ i) {
        var ext = data.available_data[i];
        var link = APP_PREFIX + "/data/" + data.id + "/" + ext;
        datalinks += "<a href=\"" + link + "\">" + ext + "</a> ";
    }
    return (
        "<tr>"
            + "<td><a onclick=\"openReader(" + data.id
            + ")\" href=\"javascript:void(0);\">"
            + "<span class=\"glyphicon glyphicon-book\" aria-hidden=\"true\"></span>"
            + "</a></td>"
            + "<td>" + data.title + "</td>"
            + "<td>" + data.author_sort + "</td>"
            + "<td>" + datalinks + "</td>"
            + "</tr>");
}

function renderBookList(data) {
    var listElem = $("#booklist");

    var innerHtml = "<table>";
    innerHtml += "<thead><tr><th class=\"col_reader_links\"></th><th class=\"col_title\">Title</th><th class=\"col_author_sort\">Author(s)</th><th class=\"col_data_links\">Data</th></tr></thead>";
    innerHtml += "<tbody>";
    for (var i = 0; i < data.length; ++ i) {
        innerHtml += genBookItemTableRow(data[i]);
    }
    innerHtml += "</tbody></table>";
    listElem.html(innerHtml);

    var table = $("#booklist table").DataTable({
        "paging": false,
        //"scrollY": "500px",
        "scrollCollapse": true,
        "order": [[1, 'asc']],
        "dom": 'Rlfrtip',
        "columnDefs":[
            {
                "targets": 0,
                "width": "20px",
                "searchable": false,
                "orderable": false
            }
        ]
    });

}

function onReadyMainPage() {
    $.ajax({
        dataType: "json",
        url: API_ROOT + "/booklist.js",
        success: function(data) {
            renderBookList(data)
        }
    });
}


function toggleDirection() {
    if (book.package.metadata.direction === "rtl") {
        book.package.metadata.direction = null;
    } else {
        book.package.metadata.direction = "rtl";
    }
}


/** on-ready function for reader page */
function onReadyReaderPage() {
    if (document.readyState == "complete") {
        window.reader = ePubReader(BOOK_URI, {
            restore: true
        });
    }
}
